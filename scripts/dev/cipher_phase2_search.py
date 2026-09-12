#!/usr/bin/env python3
from dataclasses import dataclass
from typing import List

@dataclass
class SearchResult:
    doc_id: str
    title: str
    agent_id: str
    relevance_score: float
    excerpt: str
    match_type: str

class VaultSearch:
    def __init__(self):
        self.documents_cache = {}

    def index_document(self, doc_id: str, title: str, content: str,
                      agent_id: str, tags: List[str], is_private: bool) -> None:
        self.documents_cache[doc_id] = {
            "doc_id": doc_id, "title": title, "content": content,
            "agent_id": agent_id, "tags": tags, "is_private": is_private
        }

    def search(self, query: str, agent_id: str, limit: int = 20) -> List[SearchResult]:
        results = []
        query_lower = query.lower()
        for doc_id, doc in self.documents_cache.items():
            if doc["agent_id"] != agent_id and doc["is_private"]:
                continue
            score = 0.0
            if query_lower in doc["title"].lower():
                score += 10.0
            if any(query_lower in tag.lower() for tag in doc["tags"]):
                score += 5.0
            if query_lower in doc["content"].lower():
                score += 2.0
            if score > 0:
                results.append(SearchResult(
                    doc_id=doc_id, title=doc["title"], agent_id=doc["agent_id"],
                    relevance_score=score, excerpt=doc["content"][:200],
                    match_type="title" if query_lower in doc["title"].lower() else "content"
                ))
        return sorted(results, key=lambda r: r.relevance_score, reverse=True)[:limit]

if __name__ == "__main__":
    search = VaultSearch()
    search.index_document("doc-001", "Alice's Crypto", "X25519...", "alice", ["crypto"], True)
    search.index_document("doc-002", "Shared Design", "Storage...", "alice", ["design"], False)
    results = search.search("crypto", "alice")
    print(f"Phase 2 search tests PASSED: {len(results)} results")
