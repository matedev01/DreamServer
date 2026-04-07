//! Memory store: read/write `.md` files with YAML frontmatter.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

// ---------- types ----------

/// Memory type classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryType {
    User,
    Feedback,
    Project,
    Reference,
}

impl MemoryType {
    /// Relevance boost multiplier for search scoring.
    #[must_use]
    pub const fn boost(self) -> f64 {
        match self {
            Self::Feedback => 1.2,
            Self::Project => 1.1,
            Self::User => 1.0,
            Self::Reference => 0.8,
        }
    }
}

impl std::fmt::Display for MemoryType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::User => write!(f, "user"),
            Self::Feedback => write!(f, "feedback"),
            Self::Project => write!(f, "project"),
            Self::Reference => write!(f, "reference"),
        }
    }
}

/// A single memory entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    #[serde(rename = "type")]
    pub memory_type: MemoryType,
    pub title: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub content: String,
    pub file_path: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
    #[serde(skip)]
    pub relevance_score: Option<f64>,
}

// ---------- store ----------

/// File-backed memory store.
pub struct MemoryStore {
    base_dir: PathBuf,
}

const MAX_ENTRIES: usize = 200;
const MAX_CONTENT_LEN: usize = 50_000;

impl MemoryStore {
    /// Create a new store rooted at `base_dir`. Creates the directory if missing.
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        let base_dir = base_dir.into();
        let _ = fs::create_dir_all(&base_dir);
        Self { base_dir }
    }

    /// Save a memory entry to disk as a Markdown file with YAML frontmatter.
    pub fn put(&self, entry: &MemoryEntry) -> std::io::Result<PathBuf> {
        let filename = format!("{}_{}.md", slugify(&entry.title), &entry.id);
        let path = self.base_dir.join(&filename);

        let frontmatter = format!(
            "---\n\
             id: {}\n\
             type: {}\n\
             title: {}\n\
             description: {}\n\
             created: {}\n\
             updated: {}\n\
             ---\n\n",
            entry.id,
            entry.memory_type,
            entry.title,
            entry.description,
            entry.created_at,
            entry.updated_at,
        );

        let content = if entry.content.len() > MAX_CONTENT_LEN {
            &entry.content[..MAX_CONTENT_LEN]
        } else {
            &entry.content
        };

        fs::write(&path, format!("{frontmatter}{content}"))?;
        self.update_index()?;
        Ok(path)
    }

    /// Load all memory entries from disk.
    #[must_use]
    pub fn load_all(&self) -> Vec<MemoryEntry> {
        let mut entries = Vec::new();
        let Ok(dir) = fs::read_dir(&self.base_dir) else {
            return entries;
        };

        for entry in dir.flatten().take(MAX_ENTRIES) {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            if path.file_name().and_then(|n| n.to_str()) == Some("MEMORY.md") {
                continue;
            }
            if let Some(mem) = parse_memory_file(&path) {
                entries.push(mem);
            }
        }

        entries.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        entries
    }

    /// Get a single entry by ID.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<MemoryEntry> {
        self.load_all().into_iter().find(|e| e.id == id)
    }

    /// Delete a memory entry by ID.
    pub fn delete(&self, id: &str) -> bool {
        let entries = self.load_all();
        if let Some(entry) = entries.iter().find(|e| e.id == id) {
            if let Some(ref fp) = entry.file_path {
                let _ = fs::remove_file(fp);
                let _ = self.update_index();
                return true;
            }
        }
        false
    }

    /// Rebuild the MEMORY.md index file.
    fn update_index(&self) -> std::io::Result<()> {
        let entries = self.load_all();
        let mut index = String::new();
        for entry in entries.iter().take(200) {
            let filename = entry
                .file_path
                .as_ref()
                .and_then(|p| Path::new(p).file_name())
                .and_then(|n| n.to_str())
                .unwrap_or("unknown.md");
            let desc = if entry.description.len() > 80 {
                &entry.description[..80]
            } else {
                &entry.description
            };
            index.push_str(&format!(
                "- [{}]({}) — {}\n",
                entry.title, filename, desc
            ));
        }
        fs::write(self.base_dir.join("MEMORY.md"), index)
    }
}

// ---------- parsing ----------

fn parse_memory_file(path: &Path) -> Option<MemoryEntry> {
    let text = fs::read_to_string(path).ok()?;
    let (frontmatter, content) = split_frontmatter(&text)?;
    let fields = parse_yaml_fields(&frontmatter);

    let id = fields.get("id")?.clone();
    let memory_type = match fields.get("type")?.as_str() {
        "user" => MemoryType::User,
        "feedback" => MemoryType::Feedback,
        "project" => MemoryType::Project,
        "reference" => MemoryType::Reference,
        _ => return None,
    };

    Some(MemoryEntry {
        id,
        memory_type,
        title: fields.get("title").cloned().unwrap_or_default(),
        description: fields.get("description").cloned().unwrap_or_default(),
        content: content.to_string(),
        file_path: Some(path.to_string_lossy().to_string()),
        created_at: fields
            .get("created")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0),
        updated_at: fields
            .get("updated")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0),
        relevance_score: None,
    })
}

/// Split `---\n...\n---\n` frontmatter from body content.
fn split_frontmatter(text: &str) -> Option<(&str, &str)> {
    let text = text.strip_prefix("---\n")?;
    let end = text.find("\n---\n")?;
    let frontmatter = &text[..end];
    let content = &text[end + 5..]; // skip "\n---\n"
    Some((frontmatter, content.trim_start_matches('\n')))
}

/// Minimal YAML parser: extract `key: value` pairs (single-line only).
fn parse_yaml_fields(frontmatter: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in frontmatter.lines() {
        if let Some((key, value)) = line.split_once(": ") {
            map.insert(key.trim().to_string(), value.trim().to_string());
        }
    }
    map
}

/// Convert a title to a filesystem-safe slug.
fn slugify(title: &str) -> String {
    let slug: String = title
        .to_ascii_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    let slug = slug.trim_matches('_').to_string();
    if slug.len() > 40 {
        slug[..40].to_string()
    } else {
        slug
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_frontmatter_works() {
        let text = "---\nid: abc\ntype: user\n---\nHello world";
        let (fm, content) = split_frontmatter(text).unwrap();
        assert!(fm.contains("id: abc"));
        assert_eq!(content, "Hello world");
    }

    #[test]
    fn parse_yaml_fields_extracts_pairs() {
        let fm = "id: abc123\ntype: feedback\ntitle: My Title";
        let fields = parse_yaml_fields(fm);
        assert_eq!(fields["id"], "abc123");
        assert_eq!(fields["type"], "feedback");
        assert_eq!(fields["title"], "My Title");
    }

    #[test]
    fn slugify_produces_safe_names() {
        assert_eq!(slugify("Hello World!"), "hello_world");
        assert_eq!(slugify("a".repeat(50).as_str()).len(), 40);
    }

    #[test]
    fn store_roundtrip() {
        let dir = std::env::temp_dir().join(format!("dreamforge_mem_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let store = MemoryStore::new(&dir);

        let entry = MemoryEntry {
            id: "test1234".to_string(),
            memory_type: MemoryType::User,
            title: "Test Memory".to_string(),
            description: "A test entry".to_string(),
            content: "Some content here.".to_string(),
            file_path: None,
            created_at: 1000,
            updated_at: 2000,
            relevance_score: None,
        };

        store.put(&entry).expect("put should succeed");

        let loaded = store.load_all();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].id, "test1234");
        assert_eq!(loaded[0].content, "Some content here.");

        let got = store.get("test1234").expect("get should find entry");
        assert_eq!(got.title, "Test Memory");

        // Index file should exist
        assert!(dir.join("MEMORY.md").exists());

        // Cleanup
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn store_delete() {
        let dir = std::env::temp_dir().join(format!("dreamforge_mem_del_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let store = MemoryStore::new(&dir);

        let entry = MemoryEntry {
            id: "del12345".to_string(),
            memory_type: MemoryType::Feedback,
            title: "Delete Me".to_string(),
            description: String::new(),
            content: "gone soon".to_string(),
            file_path: None,
            created_at: 100,
            updated_at: 200,
            relevance_score: None,
        };

        store.put(&entry).unwrap();
        assert_eq!(store.load_all().len(), 1);

        // Need to reload to get file_path populated
        assert!(store.delete("del12345"));
        assert_eq!(store.load_all().len(), 0);

        let _ = fs::remove_dir_all(&dir);
    }
}
