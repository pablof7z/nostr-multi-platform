//! Real-NMP driver wrapper used by every scenario.
//!
//! Builds a real `NmpApp` (the actor + kernel), pointed at the embedded
//! fixture relay, with a real local signer (generated `nostr::Keys`) and an
//! injected deterministic clock. Exposes the public driver seam the catalog
//! lists: `dispatch_action("nmp.publish", …)`, `register_event_observer`,
//! `register_raw_event_observer`, `push_interest`, `event_by_id`,
//! `event_store_handle`, and the test-support kernel-injection /
//! projection-read seams.
//!
//! NOTHING here reimplements kernel logic: events are produced with real
//! Schnorr signing, published through the real publish engine, and ingested
//! through the real relay worker → `handle_event` → `verify_and_persist`
//! chokepoint (relay path) or `dispatch_action` → publish engine (local path).
//!
//! Some toolkit affordances (`advance_clock`, `projection_json`,
//! `RawCollector`, `FixtureRelay::has_event`) are intentionally part of the
//! reusable driver surface for the not-yet-landed scenarios (PR3 contacts
//! backfill needs the clock; Workstream B/F will use the projection-read and
//! raw-tap lenses). They are `allow(dead_code)` until those scenarios land
//! rather than deleted-and-rewritten.
#![allow(dead_code)]

use std::ffi::CString;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant, SystemTime};

use nmp_core::substrate::KernelEvent;
use nmp_core::{KernelEventObserver, KindFilter, MonotonicSecondClock, RawEventObserver, SignerSource};
use nostr::nips::nip19::ToBech32;
use nostr::Keys;

use crate::relay::FixtureRelay;

/// Collects `on_kernel_event` deliveries (the app-facing observer seam — fires
/// on `Inserted|Replaced|Ephemeral` per ADR-0057). Records per-id fire counts
/// so dedup / single-fire (D4) can be asserted, and a condvar so a scenario
/// can block until an expected event arrives without a sleep-loop.
#[derive(Default)]
pub struct EventCollector {
    inner: Mutex<CollectorInner>,
    cv: Condvar,
}

#[derive(Clone)]
pub struct ObservedEvent {
    pub id: String,
    pub author: String,
    pub kind: u32,
    pub created_at: u64,
    pub content: String,
}

#[derive(Default)]
struct CollectorInner {
    /// Full ordered log of observed events.
    events: Vec<ObservedEvent>,
    /// id -> number of times the observer fired for it.
    fire_counts: std::collections::HashMap<String, u32>,
}

impl EventCollector {
    fn record(&self, ev: &KernelEvent) {
        let mut g = self.inner.lock().expect("collector lock");
        g.events.push(ObservedEvent {
            id: ev.id.clone(),
            author: ev.author.clone(),
            kind: ev.kind,
            created_at: ev.created_at,
            content: ev.content.clone(),
        });
        *g.fire_counts.entry(ev.id.clone()).or_insert(0) += 1;
        drop(g);
        self.cv.notify_all();
    }

    /// Number of times the app observer fired for `id` (0 if never).
    pub fn fire_count(&self, id: &str) -> u32 {
        self.inner
            .lock()
            .map(|g| g.fire_counts.get(id).copied().unwrap_or(0))
            .unwrap_or(0)
    }

    /// The `created_at` the observer was handed for `id`, if it fired. This is
    /// the value AFTER any projection-path clamp (D9), distinct from the stored
    /// timestamp.
    pub fn observed_created_at(&self, id: &str) -> Option<u64> {
        let g = self.inner.lock().ok()?;
        g.events.iter().find(|e| e.id == id).map(|e| e.created_at)
    }

    /// All observed ids in delivery order.
    pub fn observed_ids(&self) -> Vec<String> {
        self.inner
            .lock()
            .map(|g| g.events.iter().map(|e| e.id.clone()).collect())
            .unwrap_or_default()
    }

    /// Find the first observed event matching `pred` (e.g. by kind+author+
    /// content — used for locally-published events whose final signed id is not
    /// known at dispatch time because the actor stamps created_at and signs).
    pub fn find_match(&self, pred: impl Fn(&ObservedEvent) -> bool) -> Option<ObservedEvent> {
        let g = self.inner.lock().ok()?;
        g.events.iter().find(|e| pred(e)).cloned()
    }

    /// Block until an observed event matches `pred`, or timeout. Returns the
    /// matching event if found. Condvar-driven.
    pub fn wait_for_match(
        &self,
        timeout: Duration,
        pred: impl Fn(&ObservedEvent) -> bool,
    ) -> Option<ObservedEvent> {
        let deadline = Instant::now() + timeout;
        let mut g = self.inner.lock().expect("collector lock");
        loop {
            if let Some(e) = g.events.iter().find(|e| pred(e)) {
                return Some(e.clone());
            }
            let now = Instant::now();
            if now >= deadline {
                return None;
            }
            let (ng, _) = self
                .cv
                .wait_timeout(g, deadline - now)
                .expect("collector wait");
            g = ng;
        }
    }

    /// Block until the observer has fired for `id` at least once, or `timeout`
    /// elapses. Returns whether it fired. Condvar-driven (no busy sleep loop).
    pub fn wait_for(&self, id: &str, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut g = self.inner.lock().expect("collector lock");
        loop {
            if g.fire_counts.contains_key(id) {
                return true;
            }
            let now = Instant::now();
            if now >= deadline {
                return g.fire_counts.contains_key(id);
            }
            let (ng, _) = self
                .cv
                .wait_timeout(g, deadline - now)
                .expect("collector wait");
            g = ng;
        }
    }
}

impl KernelEventObserver for EventCollector {
    fn on_kernel_event(&self, event: &KernelEvent) {
        self.record(event);
    }
}

/// Collects raw-tap deliveries (verbatim signed frames). The raw tap fires on
/// live ingest including `Duplicate` and does NOT fire on cache-served replay,
/// so it is the lens for the parser-vs-store distinction in several scenarios.
#[derive(Default)]
pub struct RawCollector {
    counts: Mutex<std::collections::HashMap<String, u32>>,
}

impl RawCollector {
    pub fn fire_count(&self, id: &str) -> u32 {
        self.counts
            .lock()
            .map(|g| g.get(id).copied().unwrap_or(0))
            .unwrap_or(0)
    }
    pub fn total(&self) -> u32 {
        self.counts.lock().map(|g| g.values().sum()).unwrap_or(0)
    }
}

impl RawEventObserver for RawCollector {
    fn on_raw_event(&self, _kind: u32, json: &str) {
        // Extract the id from the verbatim NIP-01 JSON without depending on the
        // store types — the raw tap hands us the flat signed-event JSON.
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(json) {
            if let Some(id) = val.get("id").and_then(serde_json::Value::as_str) {
                if let Ok(mut g) = self.counts.lock() {
                    *g.entry(id.to_string()).or_insert(0) += 1;
                }
            }
        }
    }
}

/// A running NMP app pointed at a fixture relay, with a real local signer and
/// an injected monotonic clock.
pub struct Harness {
    app: *mut nmp_ffi::NmpApp,
    pub relay: FixtureRelay,
    pub keys: Keys,
    pub clock: Arc<MonotonicSecondClock>,
    pub collector: Arc<EventCollector>,
    pub raw: Arc<RawCollector>,
    /// Base wall-clock the injected clock is anchored at (so scenarios can
    /// compute "future" / "past" timestamps relative to kernel-now).
    pub base_secs: u64,
    storage_path: Option<String>,
    /// Mirror of the cumulative clock advance (the MonotonicSecondClock's
    /// offset is write-only from our side), so `now()` is cheap and local.
    advanced: AtomicU64,
}

// The pointer is owned exclusively by this Harness; methods take `&self` and
// the underlying NmpApp is Send+Sync on the ffi side.
unsafe impl Send for Harness {}
unsafe impl Sync for Harness {}

static APP_COUNTER: AtomicU64 = AtomicU64::new(0);
/// Monotonic counter giving each `barrier()` a unique sentinel + interest id.
static BARRIER_COUNTER: AtomicU64 = AtomicU64::new(0);

impl Harness {
    /// Build an in-memory app (fast scenarios). One fresh fixture relay, one
    /// generated key, clock anchored at a fixed base.
    pub fn in_memory() -> Harness {
        Self::build(None)
    }

    /// Build a persistent app at `storage_path` (cold-restart scenarios). The
    /// same path can be reused by `restart()` to prove rebuild-from-store.
    pub fn persistent(storage_path: &str) -> Harness {
        Self::build(Some(storage_path.to_string()))
    }

    fn build(storage_path: Option<String>) -> Harness {
        let relay = FixtureRelay::start();
        let keys = Keys::generate();
        // Anchor the clock at a fixed, reproducible base well in the past of any
        // "future-dated" event we stage, but far enough ahead that NIP-40
        // expiries in the past are unambiguous.
        let base_secs = 1_900_000_000u64; // ~2030; stable across runs.
        let clock = Arc::new(MonotonicSecondClock::new(
            SystemTime::UNIX_EPOCH + Duration::from_secs(base_secs),
        ));
        let collector = Arc::new(EventCollector::default());
        let raw = Arc::new(RawCollector::default());

        let app_ptr = nmp_ffi::nmp_app_new();
        assert!(!app_ptr.is_null(), "nmp_app_new returned null");
        // SAFETY: fresh non-null pointer from nmp_app_new; we own it.
        let app_ref: &nmp_ffi::NmpApp = unsafe { &*app_ptr };

        // Inject the deterministic clock BEFORE start (read at kernel
        // construction). This is the D9 / GC / NIP-40 lever.
        app_ref.set_kernel_clock_for_test(Arc::clone(&clock));

        // Register the real local signer through the documented one-door path.
        app_ref.add_signer(
            SignerSource::LocalNsec(zeroize::Zeroizing::new(
                keys.secret_key().to_bech32().expect("nsec bech32"),
            )),
            true,
        );

        // Register the app-facing observer + the raw tap.
        let _obs_id = app_ref.register_event_observer(
            Arc::clone(&collector) as Arc<dyn KernelEventObserver>,
        );
        let _raw_id = app_ref.register_raw_event_observer(
            KindFilter::from_kinds(std::iter::empty::<u32>()),
            Arc::clone(&raw) as Arc<dyn RawEventObserver>,
        );

        // Register the REAL kind:3 contacts parser + cache (ADR-0057 PR3),
        // exactly as `nmp-defaults` does — the kind:3 effect-signal path
        // (`timeline_authors` rebuild, follow-feed cache-serve) only fires when
        // the parser writes the contacts cache the kernel diffs.
        register_contacts_parser(app_ref);

        // Point the app at the fixture relay (read + write role) before start.
        app_ref.set_initial_relays_for_start(vec![
            (relay.url().to_string(), "both".to_string()),
        ]);

        // Persistent vs in-memory storage. The FFI builder takes storage via a
        // C-ABI setter; the test-support build exposes it on the actor Start
        // through the configured storage path slot.
        if let Some(ref path) = storage_path {
            set_storage_path(app_ptr, path);
        }

        // Start the actor. visible_limit / emit_hz mirror production defaults.
        nmp_ffi::nmp_app_start(app_ptr, 60, 100, 4);

        let _ = APP_COUNTER.fetch_add(1, Ordering::Relaxed);

        Harness {
            app: app_ptr,
            relay,
            keys,
            clock,
            collector,
            raw,
            base_secs,
            storage_path,
            advanced: AtomicU64::new(0),
        }
    }

    /// Borrow the underlying NmpApp.
    pub fn app(&self) -> &nmp_ffi::NmpApp {
        // SAFETY: pointer is valid for the lifetime of the Harness.
        unsafe { &*self.app }
    }

    /// Drain barrier — a push-signal-based replacement for a sleep/poll loop
    /// (D8: no polling). Stages a UNIQUE sentinel event from a fresh foreign key
    /// onto the fixture relay and opens an interest for it, then BLOCKS on the
    /// observer condvar until the sentinel is delivered. Because the actor
    /// processes relay frames and commands in order on its single thread, once
    /// the sentinel (issued after the operation under test) is observed, every
    /// earlier-issued frame — including a relay echo / supersession sibling /
    /// expiry — has already been processed by the chokepoint. So a scenario can
    /// then synchronously read the store / fire counts and assert a *negative*
    /// (e.g. "the echo did NOT re-fire") without busy-waiting.
    pub fn barrier(&self) {
        let n = BARRIER_COUNTER.fetch_add(1, Ordering::AcqRel);
        let sentinel_key = Keys::generate();
        let sentinel_pk = sentinel_key.public_key().to_hex();
        let content = format!("__barrier_sentinel_{n}__");
        let ev = build_signed_event(&sentinel_key, 1, &content, vec![], self.now());
        let id = ev.id.to_hex();
        self.relay.stage_event(&event_to_value(&ev));
        // Unique interest id derived from the counter to avoid collisions.
        self.open_interest(relay_pinned_interest(
            self.relay.url(),
            50_000 + n,
            vec![1],
            vec![sentinel_pk],
            vec![],
        ));
        // Block until the sentinel arrives (or the wait budget elapses).
        let _ = self.collector.wait_for(&id, Duration::from_secs(5));
    }

    /// Kernel-now per the injected clock.
    pub fn now(&self) -> u64 {
        self.base_secs + self.clock_advance()
    }

    fn clock_advance(&self) -> u64 {
        // The MonotonicSecondClock advance is opaque; track via now_secs would
        // need actor round-trip. We compute now relative to base + the advances
        // the scenario applied through `advance_clock`.
        self.advanced.load(Ordering::Acquire)
    }

    /// Advance the deterministic clock by `secs`. Subsequent kernel-stamped
    /// events (and GC age comparisons) observe the new time.
    pub fn advance_clock(&self, secs: u64) {
        self.clock.advance_secs(secs);
        self.advanced.fetch_add(secs, Ordering::AcqRel);
    }

    /// Dispatch a `PublishRaw` action through the real publish engine. Returns
    /// the dispatch result JSON (`{"correlation_id":...}` or `{"error":...}`).
    pub fn publish_raw(&self, kind: u32, content: &str, tags: Vec<Vec<String>>) -> String {
        let action = serde_json::json!({
            "PublishRaw": {
                "kind": kind,
                "tags": tags,
                "content": content,
                "target": "Auto"
            }
        })
        .to_string();
        self.dispatch("nmp.publish", &action)
    }

    /// Dispatch with an explicit relay target (the fixture relay) — used when a
    /// scenario needs the publish to reach a known relay deterministically.
    pub fn publish_raw_explicit(
        &self,
        kind: u32,
        content: &str,
        tags: Vec<Vec<String>>,
    ) -> String {
        let action = serde_json::json!({
            "PublishRaw": {
                "kind": kind,
                "tags": tags,
                "content": content,
                "target": { "Explicit": { "relays": [ self.relay.url() ] } }
            }
        })
        .to_string();
        self.dispatch("nmp.publish", &action)
    }

    /// Dispatch a reserved-kind profile publish via the dedicated variant.
    pub fn publish_profile(&self, fields: serde_json::Value) -> String {
        let action = serde_json::json!({ "PublishProfile": { "fields": fields } }).to_string();
        self.dispatch("nmp.publish", &action)
    }

    /// Raw `dispatch_action` through the real C-ABI entry point.
    pub fn dispatch(&self, namespace: &str, action_json: &str) -> String {
        let ns = CString::new(namespace).expect("ns cstr");
        let aj = CString::new(action_json).expect("action cstr");
        let ret = nmp_ffi::nmp_app_dispatch_action(self.app, ns.as_ptr(), aj.as_ptr());
        if ret.is_null() {
            return String::new();
        }
        // SAFETY: nmp_app_dispatch_action returns a heap CString; take ownership.
        unsafe {
            let s = std::ffi::CStr::from_ptr(ret).to_string_lossy().into_owned();
            nmp_ffi::nmp_free_string(ret);
            s
        }
    }

    /// Inject a verbatim signed event through the kernel test-support seam.
    /// NOTE: this is the LEGACY `IngestPreVerifiedEvents` path, which the
    /// kernel routes through `ingest_pre_verified_event` — NOT the ADR-0057
    /// `verify_and_persist` chokepoint. Scenarios that must validate the LANDED
    /// chokepoint use the fixture relay (`relay.stage_event` + an interest)
    /// instead; this seam is only used where a scenario explicitly tests the
    /// store/observer contract independent of the relay transport.
    pub fn inject_signed_event_json(&self, event: &nostr::Event) -> bool {
        let json = nostr::JsonUtil::as_json(event);
        let c = CString::new(json).expect("event json cstr");
        nmp_ffi::nmp_app_inject_signed_event_json(self.app, c.as_ptr())
    }

    /// Read an event from the kernel store by hex id.
    pub fn event_by_id(&self, id: &str) -> Option<KernelEvent> {
        self.app().event_by_id(id)
    }

    /// Whether the store holds `id` (persistence oracle).
    pub fn store_has(&self, id: &str) -> bool {
        self.event_by_id(id).is_some()
    }

    /// Declare the host follow-feed kinds and open the contact feed (the real
    /// `nmp_app_open_contact_feed` C-ABI verb). Sets `follow_feed_kinds` so a
    /// later kind:3 ingest rebuilds `timeline_authors` and registers per-follow
    /// `LogicalInterest`s (A9.*).
    pub fn open_contact_feed(&self, kinds: &[u32]) {
        let json = serde_json::to_string(kinds).expect("kinds json");
        let c = CString::new(json).expect("kinds cstr");
        nmp_ffi::nmp_app_open_contact_feed(self.app, c.as_ptr());
    }

    /// Run ONE bounded GC pass to a custom LRU `ceiling` via the test-support
    /// seam — drives the REAL pin-derivation + LRU eviction on the actor thread.
    pub fn run_gc(&self, ceiling: usize) -> Option<nmp_core::store::GcReport> {
        self.app().run_gc_step_for_test(ceiling)
    }

    /// The store-tier LRU pin set as lowercase-hex event ids (test-support).
    /// An event in this set is protected from GC eviction.
    pub fn pin_set(&self) -> Vec<String> {
        self.app().kernel_inspect_for_test().pin_set_hex
    }

    /// The active follow-set `timeline_authors` (test-support).
    pub fn timeline_authors(&self) -> Vec<String> {
        self.app().kernel_inspect_for_test().timeline_authors
    }

    /// The durable-store event count (test-support store-size read).
    pub fn store_count(&self) -> usize {
        self.app().kernel_inspect_for_test().store_event_count
    }

    /// Relay URLs recorded in the durable store's provenance for `id`
    /// (test-support — the codex-#11 provenance-transition lens).
    pub fn provenance_relays(&self, id: &str) -> Vec<String> {
        self.app().store_provenance_relays_for_test(id)
    }

    /// Read a registered snapshot-projection's JSON by key (test-support seam).
    pub fn projection_json(&self, key: &str) -> Option<serde_json::Value> {
        let c = CString::new(key).ok()?;
        let ret = nmp_ffi::nmp_app_read_projection_json(self.app, c.as_ptr());
        if ret.is_null() {
            return None;
        }
        // SAFETY: heap CString from the FFI seam.
        let s = unsafe {
            let s = std::ffi::CStr::from_ptr(ret).to_string_lossy().into_owned();
            nmp_ffi::nmp_free_string(ret);
            s
        };
        serde_json::from_str(&s).ok()
    }

    /// Push a logical interest so the planner emits a REQ to the fixture relay.
    /// Returns nothing; the REQ is async — pair with `collector.wait_for`.
    pub fn open_interest(&self, interest: nmp_core::planner::LogicalInterest) {
        self.app().push_interest(interest);
    }

    /// Cold-restart: free this app and build a fresh one against the SAME
    /// storage path (must be persistent) and the SAME signer key, with fresh
    /// observers/collectors. Returns the new Harness; the fixture relay is also
    /// fresh (the prior session's relay is irrelevant after restart).
    pub fn restart(self) -> Harness {
        let path = self
            .storage_path
            .clone()
            .expect("restart requires a persistent storage path");
        let keys = self.keys.clone();
        let base_secs = self.base_secs;
        // Free the old app (joins/stops the actor on Drop of the runtime).
        nmp_ffi::nmp_app_free(self.app);
        // Forget self without double-free (app already freed).
        std::mem::forget(self);
        Self::build_with_keys(Some(path), keys, base_secs)
    }

    fn build_with_keys(storage_path: Option<String>, keys: Keys, base_secs: u64) -> Harness {
        let relay = FixtureRelay::start();
        let clock = Arc::new(MonotonicSecondClock::new(
            SystemTime::UNIX_EPOCH + Duration::from_secs(base_secs),
        ));
        let collector = Arc::new(EventCollector::default());
        let raw = Arc::new(RawCollector::default());
        let app_ptr = nmp_ffi::nmp_app_new();
        assert!(!app_ptr.is_null());
        let app_ref: &nmp_ffi::NmpApp = unsafe { &*app_ptr };
        app_ref.set_kernel_clock_for_test(Arc::clone(&clock));
        app_ref.add_signer(
            SignerSource::LocalNsec(zeroize::Zeroizing::new(
                keys.secret_key().to_bech32().expect("nsec"),
            )),
            true,
        );
        let _ = app_ref
            .register_event_observer(Arc::clone(&collector) as Arc<dyn KernelEventObserver>);
        let _ = app_ref.register_raw_event_observer(
            KindFilter::from_kinds(std::iter::empty::<u32>()),
            Arc::clone(&raw) as Arc<dyn RawEventObserver>,
        );
        register_contacts_parser(app_ref);
        app_ref.set_initial_relays_for_start(vec![(relay.url().to_string(), "both".to_string())]);
        if let Some(ref path) = storage_path {
            set_storage_path(app_ptr, path);
        }
        nmp_ffi::nmp_app_start(app_ptr, 60, 100, 4);
        Harness {
            app: app_ptr,
            relay,
            keys,
            clock,
            collector,
            raw,
            base_secs,
            storage_path,
            advanced: AtomicU64::new(0),
        }
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        if !self.app.is_null() {
            nmp_ffi::nmp_app_free(self.app);
            self.app = std::ptr::null_mut();
        }
    }
}

/// Register the production kind:3 contacts parser + shared cache on `app`
/// (ADR-0057 PR3), mirroring `nmp_defaults::register_defaults`. The kernel's
/// `ContactsLookup` reader and the `Kind3Parser` writer share ONE
/// `nmp_nip01::ContactsCache`, so an ingested kind:3 transitions the cache and
/// the kernel fires the follow-feed effects (timeline_authors rebuild +
/// follow-feed cache-serve). Must be called BEFORE `nmp_app_start`.
fn register_contacts_parser(app: &nmp_ffi::NmpApp) {
    let contacts_cache = Arc::new(nmp_nip01::ContactsCache::new());
    app.set_contacts_lookup(
        Arc::clone(&contacts_cache) as Arc<dyn nmp_core::substrate::ContactsLookup>,
    );
    let kind3_parser: Arc<dyn nmp_core::substrate::IngestParser> =
        Arc::new(nmp_nip01::Kind3Parser::new(contacts_cache));
    app.register_ingest_parser(3, kind3_parser);
}

/// Set the persistent storage path via the real C-ABI setter (read at actor
/// Start). A no-op for in-memory harnesses.
fn set_storage_path(app: *mut nmp_ffi::NmpApp, path: &str) {
    let c = CString::new(path).expect("storage path cstr");
    nmp_ffi::nmp_app_set_storage_path(app, c.as_ptr());
}

/// Build a `LogicalInterest` pinned to the fixture relay so the planner emits a
/// REQ to it deterministically (no NIP-65 outbox resolution needed). `authors`
/// + `kinds` shape the filter; `id` is the stable interest id; `tag_refs` adds
/// `#e` / `#p` constraints. Tailing so post-EOSE live events also fan in.
pub fn relay_pinned_interest(
    relay_url: &str,
    id: u64,
    kinds: Vec<u32>,
    authors: Vec<String>,
    tag_refs: Vec<(String, String)>,
) -> nmp_core::planner::LogicalInterest {
    use nmp_core::planner::{InterestId, InterestLifecycle, InterestScope};
    use nmp_core::substrate::ViewDependencies;
    let deps = ViewDependencies {
        kinds,
        authors,
        ids: Vec::new(),
        tag_refs,
        projection_keys: Vec::new(),
        relay_pin: Some(relay_url.to_string()),
        limit: None,
    };
    deps.into_logical_interest(InterestId(id), InterestScope::Global, InterestLifecycle::Tailing)
}

/// Build a real Schnorr-signed event with the given key, kind, content, tags,
/// at the given created_at — for staging into the fixture relay (foreign /
/// future-dated / delete / replaceable-sibling injection).
pub fn build_signed_event(
    keys: &Keys,
    kind: u16,
    content: &str,
    tags: Vec<nostr::Tag>,
    created_at: u64,
) -> nostr::Event {
    use nostr::{EventBuilder, Kind, Timestamp};
    EventBuilder::new(Kind::from(kind), content)
        .tags(tags)
        .custom_created_at(Timestamp::from(created_at))
        .sign_with_keys(keys)
        .expect("sign event")
}

/// Convert a `nostr::Event` to the serde_json `Value` the fixture relay stores.
pub fn event_to_value(event: &nostr::Event) -> serde_json::Value {
    serde_json::from_str(&nostr::JsonUtil::as_json(event)).expect("event json")
}
