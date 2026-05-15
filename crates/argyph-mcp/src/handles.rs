use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const HANDLE_TTL: Duration = Duration::from_secs(600);
const MAX_HANDLES: usize = 512;

#[derive(Debug, Clone)]
pub struct ExpandTarget {
    pub file: String,
    pub byte_range: (u64, u64),
    pub start_line: u32,
    pub end_line: u32,
}

#[derive(Debug, Default)]
pub struct HandleStore {
    inner: Mutex<HashMap<String, (ExpandTarget, Instant)>>,
}

impl HandleStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn issue(&self, target: ExpandTarget) -> String {
        let id = format!("eh_{}", uuid::Uuid::new_v4().simple());
        let mut handles = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        handles.retain(|_, (_, issued_at)| issued_at.elapsed() < HANDLE_TTL);
        if handles.len() >= MAX_HANDLES {
            if let Some(oldest) = handles
                .iter()
                .min_by_key(|(_, (_, issued_at))| *issued_at)
                .map(|(id, _)| id.clone())
            {
                handles.remove(&oldest);
            }
        }
        handles.insert(id.clone(), (target, Instant::now()));
        id
    }

    pub fn lookup(&self, id: &str) -> Option<ExpandTarget> {
        let mut handles = self
            .inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let (target, issued_at) = handles.get(id)?;
        if issued_at.elapsed() < HANDLE_TTL {
            return Some(target.clone());
        }
        handles.remove(id);
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn issue_then_lookup_returns_target() {
        let store = HandleStore::new();
        let id = store.issue(ExpandTarget {
            file: "a.rs".into(),
            byte_range: (0, 10),
            start_line: 1,
            end_line: 2,
        });
        let Some(got) = store.lookup(&id) else {
            panic!("handle should resolve");
        };
        assert_eq!(got.file, "a.rs");
        assert_eq!(got.byte_range, (0, 10));
    }

    #[test]
    fn unknown_handle_returns_none() {
        let store = HandleStore::new();
        assert!(store.lookup("eh_deadbeef").is_none());
    }
}
