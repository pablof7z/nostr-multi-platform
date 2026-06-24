use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use nmp_nip77::SyncedItem;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CachedEvent {
    pub id: String,
    pub created_at: u64,
    pub kind: u16,
    pub pubkey: String,
    pub content: String,
    pub raw_json: String,
}

#[derive(Default, Debug, Serialize, Deserialize)]
pub struct EventCache {
    pub relay: String,
    pub filter_json: String,
    pub events: BTreeMap<String, CachedEvent>,
}

impl EventCache {
    pub fn load(path: &Path, relay: &str, filter_json: &str) -> Self {
        let loaded = fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str::<Self>(&text).ok());
        match loaded {
            Some(cache) if cache.relay == relay && cache.filter_json == filter_json => cache,
            _ => Self {
                relay: relay.to_string(),
                filter_json: filter_json.to_string(),
                events: BTreeMap::new(),
            },
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        fs::write(path, text).map_err(|e| e.to_string())
    }

    pub fn synced_items(&self) -> Vec<SyncedItem> {
        self.events
            .values()
            .filter_map(|event| {
                hex_to_32(&event.id).map(|id| SyncedItem {
                    created_at: event.created_at,
                    id,
                })
            })
            .collect()
    }

    pub fn newest(&self, limit: usize) -> Vec<CachedEvent> {
        let mut events: Vec<_> = self.events.values().cloned().collect();
        events.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(a.id.cmp(&b.id)));
        events.truncate(limit);
        events
    }
}

pub fn default_cache_path(relay: &str, filter_json: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/nip77-real-relay-cache")
        .join(format!("{:016x}.json", fnv64(relay, filter_json)))
}

fn fnv64(relay: &str, filter_json: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in relay.bytes().chain([0xff]).chain(filter_json.bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

pub fn hex_to_32(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, pair) in s.as_bytes().chunks(2).enumerate() {
        out[i] = (hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?;
    }
    Some(out)
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
