// Real thumbnails with a persistent disk cache. The previous version served
// the *original* file straight through the asset protocol — the webview
// decoded the entire full-resolution image just to shrink it visually into
// a 60px box. This generates one small copy per source file, caches it to
// disk, and reuses it on every later request instead of decoding the full
// image again.

use image::imageops::FilterType;
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

const CACHE_DIR: &str = r"S:\System\Cipher\thumbnails";
const MAX_DIMENSION: u32 = 240; // several times the ~60px display size, enough for high-DPI screens without being anywhere near "full resolution"

fn cache_dir() -> PathBuf {
    PathBuf::from(CACHE_DIR)
}

// Cache key folds in size + modified time alongside the path, so if a file
// at the same path is later replaced/edited, the key changes and the old
// thumbnail is naturally orphaned rather than silently shown as stale.
// (DefaultHasher's exact output isn't guaranteed stable across Rust
// versions — if the app is rebuilt with a different compiler, some cache
// entries may simply miss and regenerate once. Harmless, not a bug.)
fn cache_key(path: &Path, size: u64, modified: u64) -> String {
    let mut hasher = DefaultHasher::new();
    path.to_string_lossy().hash(&mut hasher);
    size.hash(&mut hasher);
    modified.hash(&mut hasher);
    format!("{:016x}.png", hasher.finish())
}

// Pure Rust HEIC/HEIF decode (the `heic` crate — no native library, same
// risk level as the `image` crate itself). Verified against the crate's
// own documented example; not run against a real .heic file here.
fn decode_heic(source: &Path) -> Result<image::DynamicImage, String> {
    let data = fs::read(source).map_err(|e| format!("can't read {}: {}", source.display(), e))?;
    let info = heic::ImageInfo::from_bytes(&data).map_err(|e| format!("heic info: {:?}", e))?;
    let buf_size = info
        .output_buffer_size(heic::PixelLayout::Rgb8)
        .ok_or_else(|| "heic: couldn't determine output buffer size".to_string())?;
    let mut buf = vec![0u8; buf_size];
    let (w, h) = heic::DecoderConfig::new()
        .decode_request(&data)
        .with_output_layout(heic::PixelLayout::Rgb8)
        .decode_into(&mut buf)
        .map_err(|e| format!("heic decode: {:?}", e))?;
    let rgb = image::RgbImage::from_raw(w, h, buf).ok_or_else(|| "heic: buffer size mismatch".to_string())?;
    Ok(image::DynamicImage::ImageRgb8(rgb))
}

// PDFium install location — a stable, explicit directory rather than
// "next to the executable" (which in dev mode is somewhere under
// target/debug, wiped by `cargo clean`). Nik places the downloaded
// pdfium.dll here once; see the setup note in Notion.
const PDFIUM_DIR: &str = r"S:\System\Cipher\pdfium";

// PDF first-page render via pdfium-render — the mature, widely-used
// binding. Deliberately NOT pdfium-bind: that crate regenerates FFI
// bindings via bindgen on every build, which needs libclang *and* a
// correctly configured MSVC/Windows SDK include path, and hit real
// failures on both counts. pdfium-render uses pre-generated bindings by
// default (no bindgen at all) and only touches the actual PDFium library
// at runtime, so a missing/misplaced DLL fails just this one thumbnail
// instead of the whole build.
fn decode_pdf_first_page(source: &Path) -> Result<image::DynamicImage, String> {
    use pdfium_render::prelude::*;

    let bindings = Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(PDFIUM_DIR))
        .map_err(|e| format!("pdfium not found at {} — see PDF setup note: {:?}", PDFIUM_DIR, e))?;
    let pdfium = Pdfium::new(bindings);

    let document = pdfium
        .load_pdf_from_file(source, None)
        .map_err(|e| format!("can't open {}: {:?}", source.display(), e))?;
    let page = document.pages().get(0).map_err(|e| format!("no first page: {:?}", e))?;

    // Modest render size — this gets shrunk to MAX_DIMENSION immediately
    // after anyway, same reasoning as the PDF DPI note used previously.
    let render_config = PdfRenderConfig::new().set_target_width(600).set_maximum_height(800);
    let rendered = page
        .render_with_config(&render_config)
        .map_err(|e| format!("pdf render: {:?}", e))?;

    rendered.as_image().map_err(|e| format!("pdf as_image: {:?}", e))
}

// First-frame video extraction via ffmpeg-next. Feature-gated off by
// default — see the video-thumbnails feature note in Cargo.toml.
#[cfg(feature = "video-thumbnails")]
fn decode_video_first_frame(source: &Path) -> Result<image::DynamicImage, String> {
    ffmpeg_next::init().map_err(|e| format!("ffmpeg init: {}", e))?;
    let mut ictx = ffmpeg_next::format::input(source).map_err(|e| format!("can't open {}: {}", source.display(), e))?;
    let input = ictx
        .streams()
        .best(ffmpeg_next::media::Type::Video)
        .ok_or_else(|| "no video stream found".to_string())?;
    let video_stream_index = input.index();
    let context_decoder = ffmpeg_next::codec::context::Context::from_parameters(input.parameters())
        .map_err(|e| format!("codec context: {}", e))?;
    let mut decoder = context_decoder.decoder().video().map_err(|e| format!("video decoder: {}", e))?;

    let mut scaler = ffmpeg_next::software::scaling::context::Context::get(
        decoder.format(),
        decoder.width(),
        decoder.height(),
        ffmpeg_next::format::Pixel::RGB24,
        decoder.width(),
        decoder.height(),
        ffmpeg_next::software::scaling::flag::Flags::BILINEAR,
    )
    .map_err(|e| format!("scaler: {}", e))?;

    for (stream, packet) in ictx.packets() {
        if stream.index() != video_stream_index {
            continue;
        }
        decoder.send_packet(&packet).map_err(|e| format!("send packet: {}", e))?;
        let mut decoded = ffmpeg_next::util::frame::video::Video::empty();
        if decoder.receive_frame(&mut decoded).is_ok() {
            let mut rgb_frame = ffmpeg_next::util::frame::video::Video::empty();
            scaler.run(&decoded, &mut rgb_frame).map_err(|e| format!("scale: {}", e))?;
            let w = rgb_frame.width();
            let h = rgb_frame.height();
            let rgb = image::RgbImage::from_raw(w, h, rgb_frame.data(0).to_vec())
                .ok_or_else(|| "video: buffer size mismatch".to_string())?;
            return Ok(image::DynamicImage::ImageRgb8(rgb));
        }
    }
    Err("no video frame could be decoded".to_string())
}

#[cfg(not(feature = "video-thumbnails"))]
fn decode_video_first_frame(_source: &Path) -> Result<image::DynamicImage, String> {
    Err("Video thumbnails aren't turned on yet — needs FFmpeg installed and the video-thumbnails Cargo feature enabled.".to_string())
}

/// Returns the path to a small cached thumbnail for the given file,
/// generating and caching it first if it doesn't exist yet. Dispatches by
/// extension: images go through the plain `image` crate as before; HEIC,
/// PDF, and video (MP4/MOV) each get their own decoder, then all converge
/// on the same resize+cache path.
pub fn get_or_create_thumbnail(source_path: &str) -> Result<String, String> {
    let source = Path::new(source_path);
    let metadata = fs::metadata(source).map_err(|e| format!("can't read {}: {}", source_path, e))?;
    let size = metadata.len();
    let modified = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let dir = cache_dir();
    fs::create_dir_all(&dir).map_err(|e| format!("can't create cache dir {}: {}", dir.display(), e))?;

    let cached_path = dir.join(cache_key(source, size, modified));
    if cached_path.is_file() {
        return Ok(cached_path.to_string_lossy().to_string());
    }

    let ext = source.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    let img = match ext.as_str() {
        "heic" | "heif" => decode_heic(source)?,
        "pdf" => decode_pdf_first_page(source)?,
        "mp4" | "mov" => decode_video_first_frame(source)?,
        _ => image::open(source).map_err(|e| format!("can't decode {}: {}", source_path, e))?,
    };
    let thumb = img.resize(MAX_DIMENSION, MAX_DIMENSION, FilterType::Triangle);
    thumb
        .save(&cached_path)
        .map_err(|e| format!("can't save thumbnail: {}", e))?;

    Ok(cached_path.to_string_lossy().to_string())
}

// "Clear cache" in Settings — wipes every generated thumbnail. Safe to call
// anytime: get_or_create_thumbnail recreates the directory and regenerates
// entries on demand.
pub fn clear_cache() -> Result<(), String> {
    let dir = cache_dir();
    if dir.is_dir() {
        fs::remove_dir_all(&dir).map_err(|e| format!("failed to clear thumbnail cache: {}", e))?;
    }
    Ok(())
}
