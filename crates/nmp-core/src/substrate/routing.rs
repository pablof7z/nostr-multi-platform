//! Routing substrate — `OutboxRouter` trait, `MailboxCache` trait, and the
//! supporting value types they exchange.
//!
//! Defined by `docs/architecture/crate-boundaries.md` §3.2, §3.3. Step 1.c +
//! 1.d of the 12-step migration. Pure additions: the kernel does not consume
//! these types yet — the existing hardwired `kernel::outbox` keeps working.
//! Step 2 creates `nmp-router` and ships the single generic `OutboxRouter`
//! impl; step 3 cuts the kernel over to `Arc<dyn OutboxRouter>`.
//!
//! ## Naming collision with `planner::MailboxCache`
//!
//! `crates/nmp-core/src/planner/compiler/mailbox.rs` already defines a trait
//! also named `MailboxCache` with a *different* shape (`get`,
//! `dm_inbox_relays`, `snapshot_all`, `generation`, `request_probe`). That
//! trait is the planner-internal compiler seam — it mixes NIP-65 kind:10002
//! lookups and NIP-17 kind:10050 lookups, which is exactly the V-40 mixing
//! the spec calls out. The substrate trait defined here is the **NIP-65-only**
//! seam the router consults. They cohabit because they live in different
//! modules (`substrate::MailboxCache` vs `planner::MailboxCache`) and never
//! `use` each other; V-40 + step 9 (planner extraction) eventually retire
//! the planner one.

use std::collections::{BTreeMap, BTreeSet};

use crate::planner::interest::LogicalInterest;
use crate::substrate::UnsignedEvent;

pub type Pubkey = String;
pub type RelayUrl = String;

// ─── RoutingSource and sub-enums ─────────────────────────────────────────────

/// NIP-65 mailbox direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Direction {
    Read,
    Write,
}

/// Sub-category for [`RoutingSource::UserConfigured`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UserConfiguredCategory {
    ActiveAccountRead,
    ActiveAccountWrite,
    Debug,
}

/// NIP-51 class routing target — the `class` part of `ClassRouted` (ADR-0020).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EventClass {
    Search,
    Draft,
    Wiki,
    /// Open-ended for NIP-51 classes not enumerated above.
    Other(String),
}

/// How the router resolved a NIP-51 class to a relay set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ClassRoutingPath {
    /// Caller populated `RoutingContext::explicit_targets`.
    Explicit,
    /// Resolved from a NIP-51 list event.
    Nip51,
}

/// App-relay lane mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AppRelayMode {
    /// Used when the author has no NIP-65 mailbox.
    Fallback,
    /// Always added to the resolved set.
    Always,
}

/// The seven routing lanes (`docs/architecture/crate-boundaries.md` §3.1).
///
/// Attached to every relay URL in a [`RoutedRelaySet`] so callers can tell
/// *why* a relay made the cut. A URL may carry multiple sources when more
/// than one lane resolved it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum RoutingSource {
    /// Lane 1 — per-author NIP-65 outbox/inbox (kind:10002).
    Nip65 { direction: Direction },
    /// Lane 2 — relay hint from an event tag.
    Hint,
    /// Lane 3 — provenance from a prior event.
    Provenance,
    /// Lane 4 — user-configured (active-account read/write, debug).
    UserConfigured(UserConfiguredCategory),
    /// Lane 5 — NIP-51 class routing (search/draft/wiki — ADR-0020).
    ClassRouted { class: EventClass, via: ClassRoutingPath },
    /// Lane 6 — operator-configured indexer relays. Always-on for kind:0,
    /// kind:3, kind:10000–19999; R+W symmetric.
    Indexer,
    /// Lane 7 — operator-configured app relays.
    AppRelay { mode: AppRelayMode },
}

// ─── BlockedRelaySet ─────────────────────────────────────────────────────────

/// Kind:10006 blocked-relay set — applied as a subtractive post-pass over
/// the routed set (`docs/architecture/crate-boundaries.md` §3.1).
#[derive(Clone, Debug, Default)]
pub struct BlockedRelaySet {
    blocked: BTreeSet<RelayUrl>,
}

impl BlockedRelaySet {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, url: RelayUrl) {
        self.blocked.insert(url);
    }

    #[must_use]
    pub fn contains(&self, url: &RelayUrl) -> bool {
        self.blocked.contains(url)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.blocked.is_empty()
    }

    #[must_use]
    pub fn iter(&self) -> impl Iterator<Item = &RelayUrl> {
        self.blocked.iter()
    }
}

// ─── SessionKeySet ───────────────────────────────────────────────────────────

/// Active-account read/write/app/indexer relay slots the router consults for
/// lanes 4, 6, and 7. Step 1.c ships the marker; later migrations fill in
/// the concrete slots as those lanes start resolving against real state.
#[derive(Clone, Copy, Debug, Default)]
pub struct SessionKeySet<'a> {
    pub active_read: &'a [RelayUrl],
    pub active_write: &'a [RelayUrl],
    pub app_relays: &'a [RelayUrl],
    pub indexer_relays: &'a [RelayUrl],
}

// ─── RoutingContext ──────────────────────────────────────────────────────────

/// Per-call context the kernel passes into the router. Crucially carries the
/// `explicit_targets` override seam (spec §3.4): when populated by a NIP
/// crate's action, the generic algorithm is skipped and the override URLs
/// are returned, attributed to the `ClassRouted` lane (minus blocked-relay
/// hits).
pub struct RoutingContext<'a> {
    pub active_account: Option<&'a Pubkey>,
    pub session_keys: SessionKeySet<'a>,
    pub mailbox_cache: &'a dyn MailboxCache,
    pub blocked_relays: &'a BlockedRelaySet,

    /// The override seam. When `Some`, the router's generic algorithm is
    /// skipped entirely and these URLs are returned attributed to
    /// [`RoutingSource::ClassRouted`] (minus blocked-relay post-filter
    /// hits). Populated by `nmp-nip17::dm_send` (recipient's kind:10050
    /// write set), `nmp-nip29` action modules (group's host relay), and
    /// `nmp-marmot` actions (MLS group relay). The router has no idea what
    /// NIP populated the field; it only knows it is present.
    pub explicit_targets: Option<&'a [RelayUrl]>,
}

// ─── RoutedRelaySet ──────────────────────────────────────────────────────────

/// Per-URL resolution attributed to the lane(s) that put it on the slice.
/// An empty `relays` map means no lane carried the event — surfaced as
/// [`RoutingError::Unroutable`] rather than silently broadcast to a fallback.
#[derive(Clone, Debug, Default)]
pub struct RoutedRelaySet {
    pub relays: BTreeMap<RelayUrl, BTreeSet<RoutingSource>>,
}

impl RoutedRelaySet {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Build from an explicit-targets slice (§3.2): every URL attributed to
    /// [`RoutingSource::ClassRouted`] with `via = Explicit`, blocked URLs
    /// dropped.
    #[must_use]
    pub fn from_explicit(urls: &[RelayUrl], blocked: &BlockedRelaySet) -> Self {
        let mut relays = BTreeMap::new();
        for url in urls {
            if blocked.contains(url) {
                continue;
            }
            relays
                .entry(url.clone())
                .or_insert_with(BTreeSet::new)
                .insert(RoutingSource::ClassRouted {
                    class: EventClass::Other(String::from("explicit")),
                    via: ClassRoutingPath::Explicit,
                });
        }
        Self { relays }
    }

    /// Insert `url` attributed to `source` (additive; multiple sources for
    /// the same URL coexist in the inner set).
    pub fn add(&mut self, url: RelayUrl, source: RoutingSource) {
        self.relays.entry(url).or_default().insert(source);
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.relays.is_empty()
    }

    #[must_use]
    pub fn urls(&self) -> impl Iterator<Item = &RelayUrl> {
        self.relays.keys()
    }
}

// ─── RoutingError ────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RoutingError {
    /// Author has no NIP-65 AND no AppRelay AND no other lane applied AND no
    /// `explicit_targets` were provided. Kernel surfaces as the
    /// `CompiledPlan::unroutable_authors` toast.
    Unroutable(Pubkey),
}

impl std::fmt::Display for RoutingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unroutable(pk) => write!(f, "unroutable author: {pk}"),
        }
    }
}

impl std::error::Error for RoutingError {}

// ─── OutboxRouter trait ──────────────────────────────────────────────────────

/// Substrate trait. Implemented by `nmp-router` (single generic algorithm).
/// NIP crates do **not** implement this trait and do **not** register
/// routing rules; they shape decisions per-call by populating
/// [`RoutingContext::explicit_targets`].
pub trait OutboxRouter: Send + Sync {
    /// Resolve relays for publishing an event. The kernel calls this BEFORE
    /// signing — `evt` is the unsigned event so the router can read its
    /// kind, tags, and author. The router must not mutate.
    fn route_publish(
        &self,
        evt: &UnsignedEvent,
        ctx: &RoutingContext<'_>,
    ) -> Result<RoutedRelaySet, RoutingError>;

    /// Resolve relays for a subscription (REQ). Discovery kinds (kind:0,
    /// kind:3, kind:10000–19999) consult the indexer lane in addition to
    /// the per-author NIP-65 read set; content kinds do not.
    fn route_subscription(
        &self,
        interest: &LogicalInterest,
        ctx: &RoutingContext<'_>,
    ) -> Result<RoutedRelaySet, RoutingError>;
}

// ─── MailboxCache trait (NIP-65 only) ────────────────────────────────────────

/// Parsed kind:10002 payload — populated by `nmp-router`'s ingest parser.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ParsedRelayList {
    pub read: Vec<RelayUrl>,
    pub write: Vec<RelayUrl>,
    pub both: Vec<RelayUrl>,
}

impl ParsedRelayList {
    /// Resolved read set: explicit reads + `both`.
    #[must_use]
    pub fn read_set(&self) -> Vec<RelayUrl> {
        let mut out = self.read.clone();
        out.extend(self.both.iter().cloned());
        out
    }

    /// Resolved write set: explicit writes + `both`.
    #[must_use]
    pub fn write_set(&self) -> Vec<RelayUrl> {
        let mut out = self.write.clone();
        out.extend(self.both.iter().cloned());
        out
    }
}

/// Substrate trait — NIP-65 (kind:10002) cache only. NIP-17's kind:10050
/// `DmRelayCache` does NOT implement this trait; it lives entirely inside
/// `nmp-nip17` and is consulted directly by the DM send action, never by
/// the router.
///
/// The trait method `upsert` takes `&self` — implementations use interior
/// mutability (the kind:10002 ingest parser is the single writer).
pub trait MailboxCache: Send + Sync {
    fn read_relays(&self, author: &Pubkey) -> Option<Vec<RelayUrl>>;
    fn write_relays(&self, author: &Pubkey) -> Option<Vec<RelayUrl>>;

    /// Default impl: known iff either the read or write set is `Some`.
    fn known(&self, author: &Pubkey) -> bool {
        self.read_relays(author).is_some() || self.write_relays(author).is_some()
    }

    /// Single writer — only called by `nmp-router`'s kind:10002 ingest path.
    /// The trait makes the contract structural rather than convention.
    fn upsert(&self, author: Pubkey, list: ParsedRelayList);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Trivial cache for substrate-trait tests. Returns whatever was
    /// last upserted; never delegates to a real backend.
    #[derive(Default)]
    struct TestMailboxCache {
        inner: Mutex<BTreeMap<Pubkey, ParsedRelayList>>,
    }

    impl MailboxCache for TestMailboxCache {
        fn read_relays(&self, author: &Pubkey) -> Option<Vec<RelayUrl>> {
            self.inner
                .lock()
                .unwrap()
                .get(author)
                .map(ParsedRelayList::read_set)
        }
        fn write_relays(&self, author: &Pubkey) -> Option<Vec<RelayUrl>> {
            self.inner
                .lock()
                .unwrap()
                .get(author)
                .map(ParsedRelayList::write_set)
        }
        fn upsert(&self, author: Pubkey, list: ParsedRelayList) {
            self.inner.lock().unwrap().insert(author, list);
        }
    }

    /// Always-unroutable router for trait shape testing.
    struct UnroutableRouter;

    impl OutboxRouter for UnroutableRouter {
        fn route_publish(
            &self,
            evt: &UnsignedEvent,
            _ctx: &RoutingContext<'_>,
        ) -> Result<RoutedRelaySet, RoutingError> {
            Err(RoutingError::Unroutable(evt.pubkey.clone()))
        }
        fn route_subscription(
            &self,
            _interest: &LogicalInterest,
            _ctx: &RoutingContext<'_>,
        ) -> Result<RoutedRelaySet, RoutingError> {
            Ok(RoutedRelaySet::new())
        }
    }

    fn ctx_with<'a>(
        cache: &'a dyn MailboxCache,
        blocked: &'a BlockedRelaySet,
        explicit: Option<&'a [RelayUrl]>,
    ) -> RoutingContext<'a> {
        RoutingContext {
            active_account: None,
            session_keys: SessionKeySet::default(),
            mailbox_cache: cache,
            blocked_relays: blocked,
            explicit_targets: explicit,
        }
    }

    fn unsigned(pubkey: &str, kind: u32) -> UnsignedEvent {
        UnsignedEvent {
            pubkey: pubkey.into(),
            kind,
            tags: vec![],
            content: String::new(),
            created_at: 0,
        }
    }

    #[test]
    fn parsed_relay_list_read_and_write_sets_include_both() {
        let parsed = ParsedRelayList {
            read: vec!["wss://r.example".into()],
            write: vec!["wss://w.example".into()],
            both: vec!["wss://b.example".into()],
        };
        assert_eq!(parsed.read_set(), vec!["wss://r.example", "wss://b.example"]);
        assert_eq!(parsed.write_set(), vec!["wss://w.example", "wss://b.example"]);
    }

    #[test]
    fn mailbox_cache_known_default_uses_read_or_write_presence() {
        let cache = TestMailboxCache::default();
        let pk: Pubkey = "alice".into();
        assert!(!cache.known(&pk));
        cache.upsert(pk.clone(), ParsedRelayList {
            read: vec!["wss://r.example".into()],
            ..ParsedRelayList::default()
        });
        assert!(cache.known(&pk));
        assert_eq!(
            cache.read_relays(&pk),
            Some(vec!["wss://r.example".into()]),
        );
    }

    #[test]
    fn routed_relay_set_from_explicit_attributes_class_routed() {
        let urls: Vec<RelayUrl> = vec!["wss://a.example".into(), "wss://b.example".into()];
        let blocked = BlockedRelaySet::new();
        let routed = RoutedRelaySet::from_explicit(&urls, &blocked);

        assert_eq!(routed.urls().count(), 2);
        for sources in routed.relays.values() {
            assert_eq!(sources.len(), 1);
            let s = sources.iter().next().unwrap();
            assert!(matches!(
                s,
                RoutingSource::ClassRouted {
                    via: ClassRoutingPath::Explicit,
                    ..
                }
            ));
        }
    }

    #[test]
    fn routed_relay_set_from_explicit_drops_blocked() {
        let urls: Vec<RelayUrl> = vec!["wss://a.example".into(), "wss://b.example".into()];
        let mut blocked = BlockedRelaySet::new();
        blocked.insert("wss://a.example".into());

        let routed = RoutedRelaySet::from_explicit(&urls, &blocked);
        let resolved: Vec<&RelayUrl> = routed.urls().collect();
        assert_eq!(resolved, vec![&"wss://b.example".to_string()]);
    }

    #[test]
    fn outbox_router_dyn_dispatch_compiles_and_returns_error() {
        let cache = TestMailboxCache::default();
        let blocked = BlockedRelaySet::new();
        let ctx = ctx_with(&cache, &blocked, None);

        let router: &dyn OutboxRouter = &UnroutableRouter;
        let evt = unsigned("alice", 1);
        let err = router.route_publish(&evt, &ctx).unwrap_err();
        assert_eq!(err, RoutingError::Unroutable("alice".into()));
    }

    #[test]
    fn routed_relay_set_add_merges_sources_per_url() {
        let mut routed = RoutedRelaySet::new();
        let url: RelayUrl = "wss://r.example".into();
        routed.add(url.clone(), RoutingSource::Hint);
        routed.add(url.clone(), RoutingSource::Nip65 { direction: Direction::Write });

        let sources = &routed.relays[&url];
        assert_eq!(sources.len(), 2);
        assert!(sources.contains(&RoutingSource::Hint));
        assert!(sources.contains(&RoutingSource::Nip65 { direction: Direction::Write }));
    }

    #[test]
    fn routing_source_ordering_is_stable() {
        // Ord on RoutingSource is derived and load-bearing — the inner
        // BTreeSet of RoutedRelaySet relies on it. Smoke-check determinism.
        let mut sources: Vec<RoutingSource> = vec![
            RoutingSource::Indexer,
            RoutingSource::Hint,
            RoutingSource::Nip65 { direction: Direction::Read },
            RoutingSource::AppRelay { mode: AppRelayMode::Fallback },
        ];
        sources.sort();
        sources.dedup();
        assert_eq!(sources.len(), 4);
    }
}
