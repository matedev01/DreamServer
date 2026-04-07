//! Keyword-based memory retrieval with type-boosted relevance scoring.

use super::store::{MemoryEntry, MemoryStore};

/// Result of a retrieval query.
#[derive(Debug)]
pub struct RetrievalResult {
    pub entries: Vec<MemoryEntry>,
}

/// Maximum characters of content returned per entry.
const MAX_CONTENT_PREVIEW: usize = 4096;

/// Default number of results to return.
const DEFAULT_LIMIT: usize = 5;

/// Stop words to filter out from queries.
const STOP_WORDS: &[&str] = &[
    "the", "a", "an", "is", "are", "was", "were", "be", "been", "being", "have", "has", "had",
    "do", "does", "did", "will", "would", "could", "should", "may", "might", "shall", "can",
    "to", "of", "in", "for", "on", "with", "at", "by", "from", "as", "into", "through",
    "and", "or", "but", "not", "no", "if", "then", "else", "when", "while", "so", "it",
    "this", "that", "these", "those", "what", "which", "who", "how", "all", "each", "every",
];

/// Retrieve relevant memories for a query string.
///
/// Extracts keywords from the query, scores each memory entry by keyword overlap
/// boosted by memory type, and returns the top results.
#[must_use]
pub fn retrieve(store: &MemoryStore, query: &str) -> RetrievalResult {
    retrieve_with_limit(store, query, DEFAULT_LIMIT)
}

/// Like [`retrieve`] but with a custom limit.
#[must_use]
pub fn retrieve_with_limit(store: &MemoryStore, query: &str, limit: usize) -> RetrievalResult {
    let query_terms = extract_keywords(query);
    let mut entries = store.load_all();

    if query_terms.is_empty() {
        // No meaningful terms — return most recent
        entries.truncate(limit);
        truncate_contents(&mut entries);
        return RetrievalResult { entries };
    }

    // Score each entry
    for entry in &mut entries {
        let score = score_entry(entry, &query_terms);
        entry.relevance_score = Some(score);
    }

    // Sort by score descending, then by updated_at descending
    entries.sort_by(|a, b| {
        let sa = a.relevance_score.unwrap_or(0.0);
        let sb = b.relevance_score.unwrap_or(0.0);
        sb.partial_cmp(&sa)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.updated_at.cmp(&a.updated_at))
    });

    // Filter to entries with non-zero score, up to limit
    entries.retain(|e| e.relevance_score.unwrap_or(0.0) > 0.0);
    entries.truncate(limit);

    // Fall back to most recent if no matches
    if entries.is_empty() {
        entries = store.load_all();
        entries.truncate(limit);
    }

    truncate_contents(&mut entries);
    RetrievalResult { entries }
}

/// Score an entry against query keywords.
fn score_entry(entry: &MemoryEntry, query_terms: &[String]) -> f64 {
    let searchable = format!(
        "{} {} {}",
        entry.title,
        entry.description,
        &entry.content[..entry.content.len().min(500)]
    )
    .to_ascii_lowercase();

    let searchable_words: Vec<&str> = searchable.split_whitespace().collect();

    let mut hits = 0;
    for term in query_terms {
        if searchable_words.iter().any(|w| w.contains(term.as_str())) {
            hits += 1;
        }
    }

    if hits == 0 {
        return 0.0;
    }

    let overlap_ratio = hits as f64 / query_terms.len().max(1) as f64;
    overlap_ratio * entry.memory_type.boost()
}

/// Extract meaningful keywords from a query string.
fn extract_keywords(query: &str) -> Vec<String> {
    let lower = query.to_ascii_lowercase();
    lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 3)
        .filter(|w| !STOP_WORDS.contains(w))
        .map(String::from)
        .collect()
}

/// Truncate content in entries to `MAX_CONTENT_PREVIEW`.
fn truncate_contents(entries: &mut [MemoryEntry]) {
    for entry in entries {
        if entry.content.len() > MAX_CONTENT_PREVIEW {
            entry.content = entry.content[..MAX_CONTENT_PREVIEW].to_string();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::store::{MemoryType, MemoryStore, MemoryEntry};

    fn test_store() -> (MemoryStore, std::path::PathBuf) {
        static COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "dreamforge_retrieval_test_{}_{}",
            std::process::id(),
            n,
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let store = MemoryStore::new(&dir);

        store
            .put(&MemoryEntry {
                id: "r1".into(),
                memory_type: MemoryType::User,
                title: "Rust expertise".into(),
                description: "User is experienced with Rust and async programming".into(),
                content: "10 years of Rust, tokio expert".into(),
                file_path: None,
                created_at: 100,
                updated_at: 200,
                relevance_score: None,
            })
            .unwrap();

        store
            .put(&MemoryEntry {
                id: "r2".into(),
                memory_type: MemoryType::Feedback,
                title: "Testing approach".into(),
                description: "Always run integration tests before committing".into(),
                content: "User prefers integration tests over unit tests".into(),
                file_path: None,
                created_at: 100,
                updated_at: 150,
                relevance_score: None,
            })
            .unwrap();

        store
            .put(&MemoryEntry {
                id: "r3".into(),
                memory_type: MemoryType::Project,
                title: "DreamForge migration".into(),
                description: "Porting Python DreamForge to Rust".into(),
                content: "Phase 1 complete, working on server".into(),
                file_path: None,
                created_at: 100,
                updated_at: 300,
                relevance_score: None,
            })
            .unwrap();

        (store, dir)
    }

    #[test]
    fn extract_keywords_filters_stop_words() {
        let kw = extract_keywords("the Rust async programming approach");
        assert!(kw.contains(&"rust".to_string()));
        assert!(kw.contains(&"async".to_string()));
        assert!(kw.contains(&"programming".to_string()));
        assert!(kw.contains(&"approach".to_string()));
        assert!(!kw.contains(&"the".to_string()));
    }

    #[test]
    fn retrieve_matches_relevant_entries() {
        let (store, dir) = test_store();

        let result = retrieve(&store, "Rust async programming");
        assert!(!result.entries.is_empty());
        // First result should be the Rust expertise entry
        assert_eq!(result.entries[0].id, "r1");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn retrieve_returns_feedback_with_boost() {
        let (store, dir) = test_store();

        let result = retrieve(&store, "testing integration approach");
        assert!(!result.entries.is_empty());
        // Feedback type gets 1.2x boost
        assert_eq!(result.entries[0].id, "r2");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn retrieve_with_no_query_returns_most_recent() {
        let (store, dir) = test_store();

        let result = retrieve(&store, "");
        assert!(!result.entries.is_empty());
        // Most recently updated should be first
        assert_eq!(result.entries[0].id, "r3");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn retrieve_limits_results() {
        let (store, dir) = test_store();

        let result = retrieve_with_limit(&store, "", 1);
        assert_eq!(result.entries.len(), 1);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
