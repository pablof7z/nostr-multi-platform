use std::collections::BTreeSet;
use std::time::Duration;

use nostr::Keys;

#[derive(Debug, Clone)]
pub struct LastRun {
    pub label: String,
    pub relays: usize,
    pub events: usize,
    pub new_events: usize,
}

#[derive(Debug)]
pub struct Session {
    pub keys: Option<Keys>,
    pub pubkey_hex: Option<String>,
    pub relays: Vec<String>,
    pub indexers: Vec<String>,
    pub follows: BTreeSet<String>,
    pub seen_ids: BTreeSet<String>,
    pub last_run: Option<LastRun>,
    pub wall: Duration,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            keys: None,
            pubkey_hex: None,
            relays: vec!["wss://relay.primal.net".into(), "wss://purplepag.es".into()],
            indexers: vec!["wss://purplepag.es".into()],
            follows: BTreeSet::new(),
            seen_ids: BTreeSet::new(),
            last_run: None,
            wall: Duration::from_secs(8),
        }
    }
}

impl Session {
    pub fn active_pubkey(&self) -> crate::Result<&str> {
        self.pubkey_hex
            .as_deref()
            .ok_or_else(|| "no active identity - run load-key or create-account".to_string())
    }

    pub fn active_keys(&self) -> crate::Result<&Keys> {
        self.keys
            .as_ref()
            .ok_or_else(|| "no signing key - run load-key or create-account".to_string())
    }
}
