//! DreamForge memory system.
//!
//! Stores persistent memories as Markdown files with YAML frontmatter.
//! Supports keyword-based retrieval with type-boosted relevance scoring.

mod retrieval;
mod store;

pub use retrieval::{retrieve, RetrievalResult};
pub use store::{MemoryEntry, MemoryStore, MemoryType};
