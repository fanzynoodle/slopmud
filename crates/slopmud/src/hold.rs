use sbc_core::LegalHoldEntry;
use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct HoldCache {
    holds: HashMap<String, LegalHoldEntry>, // name_lc -> entry
    last_index: u64,
}

impl HoldCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn last_index(&self) -> u64 {
        self.last_index
    }

    pub fn snapshot(&self) -> Vec<LegalHoldEntry> {
        let mut v = self.holds.values().cloned().collect::<Vec<_>>();
        v.sort_by(|a, b| a.name_lc.cmp(&b.name_lc));
        v
    }

    pub fn is_held(&self, name: &str) -> Option<&LegalHoldEntry> {
        let k = name.trim().to_ascii_lowercase();
        if k.is_empty() {
            return None;
        }
        self.holds.get(&k)
    }

    pub fn apply_snapshot(&mut self, index: u64, holds: Vec<LegalHoldEntry>) {
        let mut m = HashMap::new();
        for h in holds {
            let k = h.name_lc.trim().to_ascii_lowercase();
            if k.is_empty() {
                continue;
            }
            m.insert(k, h);
        }
        self.holds = m;
        self.last_index = self.last_index.max(index);
    }

    pub fn apply_upsert(&mut self, index: u64, entry: LegalHoldEntry) {
        let k = entry.name_lc.trim().to_ascii_lowercase();
        if k.is_empty() {
            return;
        }
        self.holds.insert(k, entry);
        self.last_index = self.last_index.max(index);
    }

    pub fn apply_delete(&mut self, index: u64, name_lc: &str) {
        let k = name_lc.trim().to_ascii_lowercase();
        if k.is_empty() {
            return;
        }
        self.holds.remove(&k);
        self.last_index = self.last_index.max(index);
    }
}

#[cfg(test)]
mod tests {
    use super::HoldCache;
    use sbc_core::LegalHoldEntry;

    #[test]
    fn cache_snapshot_upsert_delete() {
        let mut c = HoldCache::new();
        assert!(c.is_held("alice").is_none());

        c.apply_upsert(
            10,
            LegalHoldEntry {
                name_lc: "alice".to_string(),
                created_at_unix: 1,
                created_by: "admin".to_string(),
                reason: "test".to_string(),
            },
        );
        assert!(c.is_held("Alice").is_some());

        c.apply_delete(11, "ALICE");
        assert!(c.is_held("alice").is_none());

        let snap = vec![
            LegalHoldEntry {
                name_lc: "bob".to_string(),
                created_at_unix: 2,
                created_by: "admin".to_string(),
                reason: "".to_string(),
            },
            LegalHoldEntry {
                name_lc: "alice".to_string(),
                created_at_unix: 3,
                created_by: "admin2".to_string(),
                reason: "x".to_string(),
            },
        ];
        c.apply_snapshot(12, snap);

        let names = c
            .snapshot()
            .into_iter()
            .map(|e| e.name_lc)
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["alice".to_string(), "bob".to_string()]);
    }
}

