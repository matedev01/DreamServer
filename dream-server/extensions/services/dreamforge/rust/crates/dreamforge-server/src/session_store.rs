//! In-memory session store for DreamForge.
//!
//! Sessions are kept in memory and optionally persisted to disk.
//! Each session holds its conversation history and metadata.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use tracing::{info, warn};

/// Session status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Idle,
    Active,
    Completed,
    Errored,
    Aborted,
}

/// A conversation session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub status: SessionStatus,
    pub created_at: u64,
    pub updated_at: u64,
    pub working_directory: String,
    pub model_id: Option<String>,
    pub permission_mode: String,
    pub turn_count: u32,
    pub total_tokens_in: u64,
    pub total_tokens_out: u64,
    pub title: Option<String>,
    pub messages: Vec<serde_json::Value>,
}

impl Session {
    /// Create a new empty session.
    #[must_use]
    pub fn new(working_directory: &str, permission_mode: &str) -> Self {
        let id = uuid::Uuid::new_v4().to_string().replace('-', "");
        let now = epoch_secs();
        Self {
            id,
            status: SessionStatus::Idle,
            created_at: now,
            updated_at: now,
            working_directory: working_directory.to_string(),
            model_id: None,
            permission_mode: permission_mode.to_string(),
            turn_count: 0,
            total_tokens_in: 0,
            total_tokens_out: 0,
            title: None,
            messages: Vec::new(),
        }
    }
}

/// Thread-safe in-memory session store.
pub struct SessionStore {
    sessions: Mutex<HashMap<String, Session>>,
}

impl SessionStore {
    #[must_use]
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
        }
    }

    /// Create a new session and return it.
    pub fn create(&self, working_directory: &str, permission_mode: &str) -> Session {
        let session = Session::new(working_directory, permission_mode);
        let mut map = self.sessions.lock().expect("session lock poisoned");
        map.insert(session.id.clone(), session.clone());
        session
    }

    /// Get a session by ID.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<Session> {
        self.sessions
            .lock()
            .expect("session lock poisoned")
            .get(id)
            .cloned()
    }

    /// Update a session in the store.
    pub fn put(&self, session: Session) {
        let mut map = self.sessions.lock().expect("session lock poisoned");
        map.insert(session.id.clone(), session);
    }

    /// List all sessions sorted by updated_at descending.
    #[must_use]
    pub fn list(&self) -> Vec<Session> {
        let map = self.sessions.lock().expect("session lock poisoned");
        let mut sessions: Vec<Session> = map.values().cloned().collect();
        sessions.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        sessions
    }

    /// Delete a session by ID. Returns `true` if it existed.
    pub fn delete(&self, id: &str) -> bool {
        self.sessions
            .lock()
            .expect("session lock poisoned")
            .remove(id)
            .is_some()
    }

    /// Get the most recently updated session, or create one if none exist.
    pub fn get_or_create(&self, working_directory: &str, permission_mode: &str) -> Session {
        let map = self.sessions.lock().expect("session lock poisoned");
        if let Some(session) = map.values().max_by_key(|s| s.updated_at) {
            return session.clone();
        }
        drop(map);
        self.create(working_directory, permission_mode)
    }

    /// Save a single session to disk as JSON.
    pub fn save_session_to_disk(&self, id: &str, sessions_dir: &Path) {
        let map = self.sessions.lock().expect("session lock poisoned");
        if let Some(session) = map.get(id) {
            let path = sessions_dir.join(format!("{}.json", id));
            match serde_json::to_string_pretty(session) {
                Ok(json) => {
                    if let Err(e) = std::fs::write(&path, json) {
                        warn!("failed to save session {id}: {e}");
                    }
                }
                Err(e) => warn!("failed to serialize session {id}: {e}"),
            }
        }
    }

    /// Save all sessions to disk.
    pub fn save_all_to_disk(&self, sessions_dir: &Path) {
        let map = self.sessions.lock().expect("session lock poisoned");
        for (id, session) in map.iter() {
            let path = sessions_dir.join(format!("{id}.json"));
            match serde_json::to_string_pretty(session) {
                Ok(json) => {
                    if let Err(e) = std::fs::write(&path, json) {
                        warn!("failed to save session {id}: {e}");
                    }
                }
                Err(e) => warn!("failed to serialize session {id}: {e}"),
            }
        }
        info!("saved {} sessions to disk", map.len());
    }

    /// Load all sessions from a directory of JSON files.
    pub fn load_from_disk(sessions_dir: &Path) -> Self {
        let store = Self::new();
        let entries = match std::fs::read_dir(sessions_dir) {
            Ok(entries) => entries,
            Err(_) => return store,
        };

        let mut count = 0u32;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            match std::fs::read_to_string(&path) {
                Ok(json) => match serde_json::from_str::<Session>(&json) {
                    Ok(session) => {
                        store
                            .sessions
                            .lock()
                            .expect("session lock poisoned")
                            .insert(session.id.clone(), session);
                        count += 1;
                    }
                    Err(e) => warn!("failed to parse session from {}: {e}", path.display()),
                },
                Err(e) => warn!("failed to read session file {}: {e}", path.display()),
            }
        }

        if count > 0 {
            info!("loaded {count} sessions from disk");
        }
        store
    }

    /// Returns the path to the sessions subdirectory, creating it if needed.
    pub fn ensure_sessions_dir(data_dir: &str) -> Option<PathBuf> {
        if data_dir.is_empty() {
            return None;
        }
        let dir = PathBuf::from(data_dir).join("sessions");
        if let Err(e) = std::fs::create_dir_all(&dir) {
            warn!("failed to create sessions dir {}: {e}", dir.display());
            return None;
        }
        Some(dir)
    }
}

impl Default for SessionStore {
    fn default() -> Self {
        Self::new()
    }
}

fn epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_and_retrieve_session() {
        let store = SessionStore::new();
        let session = store.create("/workspace", "default");
        assert_eq!(session.status, SessionStatus::Idle);

        let retrieved = store.get(&session.id).expect("session should exist");
        assert_eq!(retrieved.id, session.id);
    }

    #[test]
    fn list_returns_sorted_by_updated_at() {
        let store = SessionStore::new();
        let s1 = store.create("/a", "default");
        let mut s2 = store.create("/b", "default");
        s2.updated_at = s1.updated_at + 100;
        store.put(s2.clone());

        let list = store.list();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].id, s2.id); // most recent first
    }

    #[test]
    fn delete_removes_session() {
        let store = SessionStore::new();
        let session = store.create("/workspace", "default");
        assert!(store.delete(&session.id));
        assert!(store.get(&session.id).is_none());
        assert!(!store.delete(&session.id)); // already gone
    }
}
