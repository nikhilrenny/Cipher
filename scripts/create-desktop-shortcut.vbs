Set WshShell = WScript.CreateObject("WScript.Shell")
strDesktop = WshShell.SpecialFolders("Desktop")
Set link = WshShell.CreateShortcut(strDesktop & "\Cipher.lnk")
link.TargetPath = "S:\Projects\Cipher\start-cipher.bat"
link.WorkingDirectory = "S:\Projects\Cipher"
link.IconLocation = "S:\Projects\Cipher\src-tauri\icons\icon.ico"
link.Description = "Launch Cipher"
link.Save
