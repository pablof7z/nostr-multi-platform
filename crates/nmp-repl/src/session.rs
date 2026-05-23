//! REPL session state. One struct, owned by the main thread, mutated only
//! between `req` runs. See `docs/design/nmp-repl.md` §6.
//!
//! ## Lifecycle ownership (refactor note)
//!
//! The REPL no longer hand-rolls the outbox pipeline. `req` drives the
//! *production* [`nmp_core::subs::SubscriptionLifecycle`]. For cross-`req`
//! discovery dedup (the lifecycle's `probed_mailboxes` set + the mailbox
//! cache) both the lifecycle AND its cache live on the `Session`:
//!
//! - `lifecycle` — one instance per session. Holds `probed_mailboxes` so a
//!   second `req` does not re-probe authors whose kind:10002 already arrived
//!   (or was already attempted).
//! - `mailbox_cache` — the `&dyn MailboxCache` handed to
//!   `recompile_and_diff` / `drain_tick`. Discovery REQ responses
//!   (kind:10002) are `put` here.
//!
//! They are two separate fields (not one wrapper struct) because
//! `recompile_and_diff` borrows the cache `&` while `cache.put` needs
//! `&mut` — the mutations never overlap in time, so a split borrow at the
//! call site is the cleanest ownership. `set-seed` replaces BOTH with fresh
//! instances. `refresh mailboxes` clears `probed_mailboxes` + drops the
//! cache. `refresh follows` only drops `follows_cache` (variable-expansion
//! state, independent of the outbox lifecycle).

use std::collections::{BTreeSet, HashSet};
#[cfg(feature = "mls")]
use std::collections::HashMap;
#[cfg(feature = "mls")]
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nmp_core::planner::InMemoryMailboxCache;
use nmp_core::subs::SubscriptionLifecycle;

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

pub struct Session {
    // Identity
    pub seed_hex: Option<String>,

    // Variable-expansion cache (kind:3 follows). Independent of the outbox
    // lifecycle — `$follows` resolution is a thin targeted fetch, not outbox.
    pub follows_cache: Option<BTreeSet<String>>,

    // ── Production outbox engine ─────────────────────────────────────────
    // The real lifecycle, driven by `req`. One instance per session so its
    // `probed_mailboxes` dedup survives across `req` calls.
    pub lifecycle: SubscriptionLifecycle,
    // The mailbox cache the lifecycle reads. kind:10002 discovery responses
    // land here. Persists across `req` so a second `req` is cache-warm.
    pub mailbox_cache: InMemoryMailboxCache,

    // Configuration. Re-applied onto the lifecycle at the start of each
    // `req` so config changes between `req`s take effect.
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

    // ── MLS / Marmot (bypass-kernel, direct-WebSocket) ───────────────────
    // The identity used for MLS ops (KeyPackage signing, gift-wrap, kind:0).
    // Set by `create-account` or `load-key`. Distinct from `seed_hex` (the
    // read-only diagnostic seed); `load-key`/`create-account` ALSO set
    // `seed_hex` so the prompt + `req` reflect the active identity. Kept
    // ungated so the default build's `create-account` / `load-key` can still
    // adopt an identity for `req`/`show` purposes.
    pub mls_keys: Option<nostr::Keys>,
    // The MDK-driving service (in-memory MLS store). `Arc<Mutex<…>>` because
    // `MarmotService` is `!Sync`-friendly only behind a lock and the wire
    // helpers borrow the session mutably elsewhere. Gated: pulls in
    // `nmp-marmot`.
    #[cfg(feature = "mls")]
    pub mls_service: Option<Arc<Mutex<nmp_marmot::service::MarmotService>>>,
    // Pending welcomes keyed by gift-wrap (kind:1059) event id hex →
    // (the original gift-wrap Event, group_name, inviter_npub). Only the
    // gated `mls_*` commands populate or read this map.
    #[cfg(feature = "mls")]
    pub mls_pending_welcomes: HashMap<String, (nostr::Event, String, String)>,
}

impl Default for Session {
    fn default() -> Self {
        Self {
            seed_hex: None,
            follows_cache: None,
            lifecycle: SubscriptionLifecycle::new(),
            mailbox_cache: InMemoryMailboxCache::new(),
            // Single canonical indexer (original default). `set-indexer`
            // overrides it. The instrumented `fetch_follows` shows the
            // connect attempt + its terminal status (EOSE / CLOSED / AUTH /
            // error) so a rate-limited or dead indexer is visible, never
            // silent. No hardcoded fallback list — relay choice is operator
            // config, not a baked-in default.
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
            mls_keys: None,
            #[cfg(feature = "mls")]
            mls_service: None,
            #[cfg(feature = "mls")]
            mls_pending_welcomes: HashMap::new(),
        }
    }
}

impl Session {
    #[must_use] 
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the lifecycle + mailbox cache with fresh instances. Called by
    /// `set-seed` (new identity → probed set and cache are meaningless).
    pub fn reset_lifecycle(&mut self) {
        self.lifecycle = SubscriptionLifecycle::new();
        self.mailbox_cache = InMemoryMailboxCache::new();
    }

    /// Drop just the mailbox cache, keeping the lifecycle instance (so a
    /// preceding `clear_probed_mailboxes()` stays in effect). `refresh
    /// mailboxes` uses this: the next `req` re-probes every still-unknown
    /// author against a fresh cache.
    pub fn reset_lifecycle_cache_only(&mut self) {
        self.mailbox_cache = InMemoryMailboxCache::new();
    }

    /// Short seed label for the prompt — `seed=npub1l2v…` (design §8.4) or
    /// "no-seed" if unset. Falls back to a hex prefix if npub encoding fails.
    #[must_use]
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
