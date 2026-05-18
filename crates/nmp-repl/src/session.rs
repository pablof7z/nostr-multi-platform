//! REPL session state. One struct, owned by the main thread, mutated only
//! between `req` runs. See `docs/design/nmp-repl.md` §6.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::time::Duration;

use nmp_core::planner::MailboxSnapshot;

/// Summary of the last `req` execution, surfaced via `show state`.
#[derive(Clone, Debug, Default)]
pub struct RunSummary {
    pub command_line: String,
    pub relays_used: usize,
    pub authors_on_wire: usize,
    pub unroutable: usize,
    pub events_total: u64,
    pub events_new: u64,
    pub wall: Duration,
}

#[derive(Debug)]
pub struct Session {
    // Identity
    pub seed_hex: Option<String>,

    // Discovery caches
    pub follows_cache: Option<BTreeSet<String>>,
    pub mailbox_cache: BTreeMap<String, MailboxSnapshot>,

    // Configuration
    pub indexer_relays: Vec<String>,
    pub app_relays: Vec<String>,
    pub dead_relays: BTreeSet<String>,
    pub max_connections: usize,
    pub max_per_user: usize,
    pub wall: Duration,

    // Diagnostic state
    pub seen_ids: HashSet<String>,
    pub last_run: Option<RunSummary>,

    // Output modes
    pub verbose: bool,
    pub json: bool,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            seed_hex: None,
            follows_cache: None,
            mailbox_cache: BTreeMap::new(),
            indexer_relays: vec!["wss://purplepag.es".to_string()],
            app_relays: Vec::new(),
            dead_relays: BTreeSet::new(),
            max_connections: 30,
            max_per_user: 2,
            wall: Duration::from_secs(20),
            seen_ids: HashSet::new(),
            last_run: None,
            verbose: false,
            json: false,
        }
    }
}

impl Session {
    pub fn new() -> Self {
        Self::default()
    }

    /// Short seed label for the prompt — `seed=npub1l2v…` (design §8.4) or
    /// "no-seed" if unset. Falls back to a hex prefix if npub encoding fails.
    pub fn prompt_label(&self) -> String {
        match &self.seed_hex {
            Some(hex) => match nmp_core::nip19::encode_npub(hex) {
                Ok(npub) => format!("seed={}…", &npub[..npub.len().min(12)]),
                Err(_) => format!("seed={}…", &hex[..hex.len().min(8)]),
            },
            None => "no-seed".to_string(),
        }
    }
}
