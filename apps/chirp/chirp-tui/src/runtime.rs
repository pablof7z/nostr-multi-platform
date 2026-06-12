use std::cell::Cell;
use std::collections::{HashMap, VecDeque};
use std::ffi::{CStr, CString};
use std::ptr;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

use nmp_app_chirp::ffi::{nmp_app_chirp_register_dm_inbox, nmp_app_chirp_register_follow_list};
use nmp_app_chirp::{
    follow_spec, nmp_app_chirp_close_author_feed, nmp_app_chirp_close_thread_feed,
    nmp_app_chirp_identity_restore, nmp_app_chirp_open_author_feed, nmp_app_chirp_open_home_feed,
    nmp_app_chirp_open_thread_feed, nmp_app_chirp_register, nmp_app_chirp_unregister,
    nmp_marmot_unregister, nmp_signer_broker_init, publish_note_action, react_spec, unfollow_spec,
    ChirpHandle, MarmotHandle, NmpRegisterStatus,
};
use nmp_core::substrate::IngestParser;
use nmp_core::store::VerifiedEvent;
use nmp_core::tags::Nip10Refs;
use nmp_nip01::NoteRecord;

use crate::app::ReplyTarget;
use nmp_ffi::{
    nmp_app_claim_profile, nmp_app_dispatch_action, nmp_app_free, nmp_app_load_older_feed,
    nmp_app_release_profile, nmp_app_start, nmp_free_string, NmpApp,
};
use serde_json::{json, Value};

use crate::bridge::{self, NmpEvent, NmpUpdateBridge};
use crate::Result;

const VISIBLE_AUTHOR_PROFILE_CONSUMER_PREFIX: &str = "chirp-tui.visible-author";
const VISIBLE_NOTE_RELATIONS_CONSUMER_PREFIX: &str = "chirp-tui.visible-note";

/// Slot key for the all-kinds raw-event cache parser. Must be globally unique
/// across crates (reverse-domain convention).
const RAW_CACHE_SLOT: &str = "chirp-tui.raw-cache";

/// Maximum number of raw-event JSON entries kept in the debug cache.
///
/// Older entries (by insertion order) are evicted when the cap is reached.
/// 4096 entries × ~512 bytes average ≈ 2 MiB worst-case footprint, which is
/// negligible on desktop but keeps the debug modal from growing without bound
/// during long sessions.
const RAW_CACHE_CAP: usize = 4096;

/// Bounded insertion-order evicting cache for raw-event JSON strings.
///
/// Maintains insertion order in a `VecDeque` so the oldest id can be evicted
/// in O(1) when `cap` is reached. The `HashMap` provides O(1) lookup by id.
/// Both structures are always kept in sync: an id is present in `map` iff it
/// is present in `order`.
///
/// No external LRU crate is added — the workspace already avoids that dep for
/// this purpose. "Insertion-order" (not "least-recently-used") eviction is the
/// right policy here: the debug modal is opened infrequently, and FIFO keeps
/// the most-recently-ingested events available regardless of access pattern.
struct BoundedRawCache {
    map: HashMap<String, String>,
    order: VecDeque<String>,
    cap: usize,
}

impl BoundedRawCache {
    fn new(cap: usize) -> Self {
        Self {
            map: HashMap::with_capacity(cap),
            order: VecDeque::with_capacity(cap),
            cap,
        }
    }

    /// Insert `(id, json)`. If the entry already exists it is updated in-place
    /// (position in `order` is unchanged). If the cache is at capacity the
    /// oldest inserted entry is evicted before the new one is inserted.
    fn insert(&mut self, id: String, json: String) {
        if self.map.contains_key(&id) {
            // Update existing entry without changing eviction order.
            self.map.insert(id, json);
            return;
        }
        if self.map.len() >= self.cap {
            if let Some(oldest) = self.order.pop_front() {
                self.map.remove(&oldest);
            }
        }
        self.order.push_back(id.clone());
        self.map.insert(id, json);
    }

    fn get(&self, id: &str) -> Option<&String> {
        self.map.get(id)
    }

    fn len(&self) -> usize {
        self.map.len()
    }
}

/// Caches verbatim NIP-01 wire-format event JSON (with `sig`) keyed by event
/// id, populated via the `IngestParser` seam so cache-served events (ADR-0045)
/// populate the debug modal on second launch too.
///
/// Behaviour note: because the `EventIngestDispatcher` fires on cache-serve as
/// well as live network ingest (since PR-1/#1137 and PR-2/#1145), this parser
/// also captures events that the kernel serves from its local store on cold
/// start, which actually *improves* the "View raw event" modal vs the prior
/// live-only `RawEventObserver` approach.
///
/// The backing cache is bounded at [`RAW_CACHE_CAP`] entries (insertion-order
/// eviction) to prevent unbounded memory growth during long sessions.
///
/// D8-clean: `parse` holds the mutex only for a single bounded insert.
struct RawCacheIngestParser {
    cache: Arc<Mutex<BoundedRawCache>>,
}

impl IngestParser for RawCacheIngestParser {
    fn parse(&self, evt: &VerifiedEvent) {
        // Re-serialize the already-verified event to its canonical wire JSON.
        // serde_json::to_string is infallible on a well-typed struct; the
        // `if let` guards against the (impossible in practice) error path.
        if let Ok(json) = serde_json::to_string(evt.raw()) {
            if let Ok(mut guard) = self.cache.lock() {
                guard.insert(evt.raw().id.clone(), json);
            }
        }
    }
}

pub struct AppRuntime {
    app: *mut NmpApp,
    chirp: *mut ChirpHandle,
    pub(crate) marmot: Cell<*mut MarmotHandle>,
    update_bridge: Option<Box<NmpUpdateBridge>>,
    raw_event_cache: Arc<Mutex<BoundedRawCache>>,
}

impl AppRuntime {
    #[must_use]
    pub fn new() -> Result<(Self, Receiver<NmpEvent>)> {
        let app = nmp_ffi::nmp_app_new();
        if app.is_null() {
            return Err("nmp_app_new returned null".to_string());
        }
        nmp_signer_broker_init(app);

        nmp_ffi::nmp_app_set_capability_callback(
            app,
            ptr::null_mut(),
            Some(crate::keyring::keyring_handler),
        );

        // V-73: nmp_app_chirp_register now returns a status code; the handle is
        // written through the out-parameter.  Passing null viewer_pubkey (no
        // viewer set at startup) always succeeds.
        let mut chirp: *mut ChirpHandle = ptr::null_mut();
        let register_status = nmp_app_chirp_register(app, ptr::null(), &mut chirp);
        if register_status != NmpRegisterStatus::Ok as u32 || chirp.is_null() {
            nmp_app_free(app);
            return Err(format!(
                "nmp_app_chirp_register failed (status={register_status})"
            ));
        }

        let (mut bridge, rx) = NmpUpdateBridge::channel();
        NmpUpdateBridge::register(app, &mut bridge);
        nmp_app_chirp_register_dm_inbox(app);
        nmp_app_chirp_register_follow_list(app, ptr::null());

        // Register an all-kinds IngestParser before nmp_app_start so every
        // accepted inbound event (cache-served or live) is cached by id for
        // the "View raw event" modal. The slot-keyed replace allows a future
        // account-switch to install a fresh cache without evicting unrelated
        // parsers. D8-clean: parse() holds the lock only for a single bounded insert.
        let raw_event_cache: Arc<Mutex<BoundedRawCache>> =
            Arc::new(Mutex::new(BoundedRawCache::new(RAW_CACHE_CAP)));
        // SAFETY: `app` is a valid, non-null pointer from `nmp_app_new`.
        // The borrow is not held past this statement.
        unsafe { &*app }.replace_ingest_parser_range(
            0..u32::MAX,
            RAW_CACHE_SLOT,
            Arc::new(RawCacheIngestParser {
                cache: Arc::clone(&raw_event_cache),
            }),
        );

        let db_dir = crate::keyring::chirp_data_dir()
            .map(|p| p.join("marmot"))
            .and_then(|p| std::fs::create_dir_all(&p).ok().map(|_| p));
        let marmot = db_dir.and_then(|dir| {
            let dir_c = CString::new(dir.to_string_lossy().as_ref()).ok()?;
            let h = nmp_app_chirp_identity_restore(app, dir_c.as_ptr(), ptr::null());
            if h.is_null() {
                None
            } else {
                Some(h)
            }
        });
        let initial_marmot = marmot.unwrap_or(ptr::null_mut());

        nmp_app_start(app, 0, 200, 10);
        nmp_app_chirp_open_home_feed(app);

        Ok((
            Self {
                app,
                chirp,
                marmot: Cell::new(initial_marmot),
                update_bridge: Some(bridge),
                raw_event_cache,
            },
            rx,
        ))
    }

    pub fn add_relay(&self, url: &str, role: &str) -> Result<()> {
        let url = CString::new(url).map_err(|_| "relay URL contains NUL byte".to_string())?;
        let role = CString::new(role).map_err(|_| "relay role contains NUL byte".to_string())?;
        nmp_ffi::nmp_app_add_relay(self.app, url.as_ptr(), role.as_ptr());
        Ok(())
    }

    pub fn open_thread(&self, event_id: &str) -> Result<()> {
        // M2 (ADR-0042 §5.1, V-112): use the Chirp flat-feed seam instead of the
        // deleted `nmp_app_open_thread` → `OpenThread` kernel machinery.
        self.with_cstr(event_id, |c| {
            nmp_app_chirp_open_thread_feed(self.app, c.as_ptr())
        })
    }

    pub fn close_thread(&self, event_id: &str) -> Result<()> {
        self.with_cstr(event_id, |c| {
            nmp_app_chirp_close_thread_feed(self.app, c.as_ptr())
        })
    }

    pub fn open_author(&self, pubkey: &str) -> Result<()> {
        // M2 (ADR-0042 §5.1, V-112): use the Chirp flat-feed seam instead of the
        // deleted `nmp_app_open_author` → `OpenAuthor` kernel machinery.
        self.with_cstr(pubkey, |c| {
            nmp_app_chirp_open_author_feed(self.app, c.as_ptr())
        })
    }

    pub fn close_author(&self, pubkey: &str) -> Result<()> {
        self.with_cstr(pubkey, |c| {
            nmp_app_chirp_close_author_feed(self.app, c.as_ptr())
        })
    }

    pub fn claim_visible_author_profile(&self, pubkey: &str) -> Result<()> {
        self.with_visible_author_profile_args(pubkey, |pubkey, consumer| {
            // F-TTL — claiming a visible author profile is a background /
            // on-render claim, so force = 0 (the lazy, TTL-gated path).
            nmp_app_claim_profile(self.app, pubkey.as_ptr(), consumer.as_ptr(), 0);
        })
    }

    pub fn release_visible_author_profile(&self, pubkey: &str) -> Result<()> {
        self.with_visible_author_profile_args(pubkey, |pubkey, consumer| {
            nmp_app_release_profile(self.app, pubkey.as_ptr(), consumer.as_ptr());
        })
    }

    pub fn claim_visible_note_relation_counts(&self, event_id: &str) -> Result<()> {
        self.dispatch_visible_note_relations("claim", event_id)
    }

    pub fn release_visible_note_relation_counts(&self, event_id: &str) -> Result<()> {
        self.dispatch_visible_note_relations("release", event_id)
    }

    pub fn publish_note(&self, content: &str, reply_to: Option<&ReplyTarget>) -> Result<String> {
        // Reconstruct the minimal NoteRecord the NIP-10 reply builder needs.
        // The home-feed projection carries the parent's author/content but not
        // its own Nip10Refs, so `refs` defaults to empty: the builder then
        // treats this parent as the thread root (correct for top-level replies,
        // best-effort for deep threads). The shared `publish_note_action` is
        // the single source of truth for the PublishRaw{kind:1} envelope and
        // the marked-form reply / `p` re-notification tags.
        let record = reply_to.map(|t| NoteRecord {
            event_id: t.id.clone(),
            author: t.author_pubkey.clone(),
            created_at: t.created_at,
            content: t.content.clone(),
            refs: Nip10Refs::default(),
        });
        let (namespace, action) = publish_note_action(content, record.as_ref())?;
        self.dispatch_action(&namespace, &action)
    }

    pub fn react(&self, event_id: &str, reaction: &str) -> Result<String> {
        let spec = react_spec(event_id, reaction);
        self.dispatch_action(&spec.namespace, &spec.body_json)
    }

    pub fn follow(&self, pubkey: &str, add: bool) -> Result<String> {
        let spec = if add {
            follow_spec(pubkey)
        } else {
            unfollow_spec(pubkey)
        };
        self.dispatch_action(&spec.namespace, &spec.body_json)
    }

    pub fn ack_action_stage(&self, correlation_id: &str) -> Result<()> {
        self.with_cstr(correlation_id, |c| {
            nmp_ffi::nmp_app_ack_action_stage(self.app, c.as_ptr())
        })
    }

    pub fn chirp_load_older_timeline(&self) {
        let key = CString::new("nmp.feed.home").expect("static feed key has no NUL byte");
        nmp_app_load_older_feed(self.app, key.as_ptr());
    }

    pub fn dispatch_action_value(&self, namespace: &str, action: &Value) -> Result<String> {
        self.dispatch_action(namespace, &action.to_string())
    }

    pub(crate) fn app_ptr(&self) -> *mut NmpApp {
        self.app
    }

    pub(crate) fn dispatch_action(&self, namespace: &str, action_json: &str) -> Result<String> {
        let namespace = CString::new(namespace)
            .map_err(|_| "action namespace contains NUL byte".to_string())?;
        let action =
            CString::new(action_json).map_err(|_| "action JSON contains NUL byte".to_string())?;
        let ptr = nmp_app_dispatch_action(self.app, namespace.as_ptr(), action.as_ptr());
        if ptr.is_null() {
            return Err("action dispatch returned null".to_string());
        }
        let text = unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned();
        nmp_free_string(ptr);
        let value: Value = serde_json::from_str(&text)
            .map_err(|e| format!("action dispatch returned invalid JSON: {e}"))?;
        parse_dispatch_envelope(&value)
    }

    pub(crate) fn with_cstr<T>(&self, value: &str, f: impl FnOnce(&CString) -> T) -> Result<T> {
        let c = CString::new(value).map_err(|_| "string contains NUL byte".to_string())?;
        Ok(f(&c))
    }

    fn with_visible_author_profile_args(
        &self,
        pubkey: &str,
        f: impl FnOnce(&CString, &CString),
    ) -> Result<()> {
        if self.app.is_null() {
            return Err("runtime app is not available".to_string());
        }
        let consumer_id = visible_author_profile_consumer_id(pubkey)?;
        let pubkey = CString::new(pubkey).map_err(|_| "pubkey contains NUL byte".to_string())?;
        let consumer_id = CString::new(consumer_id)
            .map_err(|_| "profile consumer id contains NUL byte".to_string())?;
        f(&pubkey, &consumer_id);
        Ok(())
    }

    /// Return the verbatim NIP-01 wire-format JSON for `event_id` (including
    /// `tags` and `sig`), or `None` if the event was evicted from the bounded
    /// cache or arrived before the parser was registered.
    pub fn raw_event_json(&self, event_id: &str) -> Option<String> {
        self.raw_event_cache.lock().ok()?.get(event_id).cloned()
    }

    fn dispatch_visible_note_relations(&self, op: &str, event_id: &str) -> Result<()> {
        if self.app.is_null() {
            return Err("runtime app is not available".to_string());
        }
        let consumer_id = visible_note_relations_consumer_id(event_id)?;
        let action = json!({
            "op": op,
            "event_id": event_id,
            "consumer_id": consumer_id,
        });
        self.dispatch_action_value("nmp.nip01.visible_note_relations", &action)
            .map(|_| ())
    }
}

fn parse_dispatch_envelope(value: &Value) -> Result<String> {
    if let Some(error) = value.get("error").and_then(Value::as_str) {
        return Err(error.to_string());
    }
    value
        .get("correlation_id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "action dispatch envelope missing correlation_id".to_string())
}

impl Drop for AppRuntime {
    fn drop(&mut self) {
        if !self.app.is_null() {
            bridge::unregister(self.app);
        }
        self.update_bridge.take();
        if !self.chirp.is_null() {
            nmp_app_chirp_unregister(self.chirp);
            self.chirp = ptr::null_mut();
        }
        if !self.marmot.get().is_null() {
            nmp_marmot_unregister(self.marmot.get());
            self.marmot.set(ptr::null_mut());
        }
        if !self.app.is_null() {
            nmp_app_free(self.app);
            self.app = ptr::null_mut();
        }
    }
}

fn visible_author_profile_consumer_id(pubkey: &str) -> Result<String> {
    validate_hex64("pubkey", pubkey)?;
    Ok(format!("{VISIBLE_AUTHOR_PROFILE_CONSUMER_PREFIX}:{pubkey}"))
}

fn visible_note_relations_consumer_id(event_id: &str) -> Result<String> {
    validate_hex64("event id", event_id)?;
    Ok(format!(
        "{VISIBLE_NOTE_RELATIONS_CONSUMER_PREFIX}:{event_id}"
    ))
}

fn validate_hex64(label: &str, value: &str) -> Result<()> {
    if value.len() == 64 && value.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(format!("{label} must be 64 hex characters"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::store::{RawEvent, VerifiedEvent};

    const ALICE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const EVENT: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    /// Build a `VerifiedEvent` from minimal valid-looking hex strings.
    fn make_event(id: &str, kind: u32) -> VerifiedEvent {
        VerifiedEvent::from_raw_unchecked(RawEvent {
            id: id.to_string(),
            pubkey: "11".repeat(32),
            created_at: 0,
            kind,
            tags: Vec::new(),
            content: String::new(),
            sig: "22".repeat(64),
        })
    }

    // ── BoundedRawCache unit tests ───────────────────────────────────────────

    /// Proves basic insert+get roundtrip.
    #[test]
    fn bounded_raw_cache_insert_get() {
        let mut c = BoundedRawCache::new(4);
        c.insert("a".to_string(), "json-a".to_string());
        assert_eq!(c.get("a"), Some(&"json-a".to_string()));
        assert_eq!(c.len(), 1);
    }

    /// Proves that duplicate inserts update the value but do not grow the cache
    /// and do not change eviction order (the original position is kept).
    #[test]
    fn bounded_raw_cache_update_in_place() {
        let mut c = BoundedRawCache::new(4);
        c.insert("a".to_string(), "v1".to_string());
        c.insert("b".to_string(), "b".to_string());
        c.insert("a".to_string(), "v2".to_string()); // update, not duplicate insert
        assert_eq!(c.get("a"), Some(&"v2".to_string()));
        assert_eq!(c.len(), 2, "no phantom growth on update");
        // Eviction order: "a" was inserted first. After "a" is updated in-place
        // its position in the eviction queue stays first. Fill to cap+1 to
        // trigger eviction and confirm "a" (oldest by insertion) is evicted.
        c.insert("c".to_string(), "c".to_string());
        c.insert("d".to_string(), "d".to_string());
        // Now at cap (4). One more insert triggers eviction of "a" (inserted first).
        c.insert("e".to_string(), "e".to_string());
        assert_eq!(c.len(), 4, "cap respected after eviction");
        assert!(c.get("a").is_none(), "oldest entry (a) must be evicted");
        assert!(c.get("b").is_some(), "b must survive");
        assert!(c.get("e").is_some(), "newest entry must be present");
    }

    /// Proves that the oldest entry is evicted when the cap is reached.
    #[test]
    fn bounded_raw_cache_evicts_oldest_at_cap() {
        let cap = 4;
        let mut c = BoundedRawCache::new(cap);
        for i in 0..cap {
            c.insert(format!("id-{i}"), format!("json-{i}"));
        }
        assert_eq!(c.len(), cap);
        // Insert one more — evicts "id-0" (oldest).
        c.insert("id-new".to_string(), "json-new".to_string());
        assert_eq!(c.len(), cap, "len stays at cap after eviction");
        assert!(c.get("id-0").is_none(), "oldest entry must be evicted");
        assert!(c.get("id-1").is_some(), "next-oldest must survive");
        assert!(c.get("id-new").is_some(), "new entry must be present");
    }

    // ── RawCacheIngestParser unit tests ──────────────────────────────────────

    fn make_parser(cap: usize) -> (RawCacheIngestParser, Arc<Mutex<BoundedRawCache>>) {
        let cache = Arc::new(Mutex::new(BoundedRawCache::new(cap)));
        let parser = RawCacheIngestParser {
            cache: Arc::clone(&cache),
        };
        (parser, cache)
    }

    /// Proves that an event dispatched through `RawCacheIngestParser::parse`
    /// lands in the shared cache under its event id.
    #[test]
    fn raw_cache_parser_stores_event_json_by_id() {
        let (parser, cache) = make_parser(16);

        let evt = make_event("00".repeat(32).as_str(), 1);
        parser.parse(&evt);

        let guard = cache.lock().unwrap();
        let json = guard.get(&"00".repeat(32)).expect("event stored in cache");
        // The stored JSON round-trips to the same event id.
        let v: serde_json::Value = serde_json::from_str(json).expect("stored JSON is valid");
        assert_eq!(
            v["id"].as_str().unwrap(),
            "00".repeat(32),
            "id in stored JSON must match the event id key"
        );
    }

    /// Proves that multiple distinct events are stored without collision.
    #[test]
    fn raw_cache_parser_stores_multiple_events() {
        let (parser, cache) = make_parser(16);

        let id_a = "aa".repeat(32);
        let id_b = "bb".repeat(32);
        parser.parse(&make_event(&id_a, 1));
        parser.parse(&make_event(&id_b, 10_050));

        let guard = cache.lock().unwrap();
        assert!(guard.get(&id_a).is_some(), "event A stored");
        assert!(guard.get(&id_b).is_some(), "event B stored");
        assert_eq!(guard.len(), 2, "exactly two entries");
    }

    /// Proves that different kinds are all stored (all-kinds coverage).
    #[test]
    fn raw_cache_parser_accepts_any_kind() {
        let (parser, cache) = make_parser(16);

        for (i, kind) in [0u32, 1, 1059, 10_002, 30_023].iter().enumerate() {
            let id = format!("{:02x}", i).repeat(32);
            parser.parse(&make_event(&id, *kind));
        }

        let guard = cache.lock().unwrap();
        assert_eq!(guard.len(), 5, "all-kinds events stored");
    }

    /// Proves that the parser evicts the oldest entry when cap is reached,
    /// keeping the cache bounded at `RAW_CACHE_CAP` entries in production.
    /// Uses a smaller cap (4) so the test does not insert 4096 events.
    #[test]
    fn raw_cache_parser_evicts_oldest_when_cap_reached() {
        let cap: usize = 4;
        let (parser, cache) = make_parser(cap);

        // Fill to cap.
        for i in 0..cap {
            let id = format!("{:064x}", i);
            parser.parse(&make_event(&id, 1));
        }
        assert_eq!(cache.lock().unwrap().len(), cap);

        // Insert one more — should evict id 0 (oldest).
        let newest_id = format!("{:064x}", cap);
        parser.parse(&make_event(&newest_id, 1));

        let guard = cache.lock().unwrap();
        assert_eq!(guard.len(), cap, "cache stays at cap after eviction");
        let oldest_id = format!("{:064x}", 0u64);
        assert!(
            guard.get(&oldest_id).is_none(),
            "oldest entry must be evicted"
        );
        assert!(
            guard.get(&newest_id).is_some(),
            "newest entry must be present after eviction"
        );
    }

    #[test]
    fn visible_author_profile_consumer_id_is_stable() {
        assert_eq!(
            visible_author_profile_consumer_id(ALICE).unwrap(),
            format!("{VISIBLE_AUTHOR_PROFILE_CONSUMER_PREFIX}:{ALICE}")
        );
    }

    #[test]
    fn visible_author_profile_claims_reject_invalid_pubkeys() {
        let (runtime, _rx) = AppRuntime::new().expect("runtime starts without live relays");

        assert_eq!(
            runtime.claim_visible_author_profile("not-a-pubkey"),
            Err("pubkey must be 64 hex characters".to_string())
        );
        assert_eq!(
            runtime.release_visible_author_profile("not-a-pubkey"),
            Err("pubkey must be 64 hex characters".to_string())
        );
    }

    #[test]
    fn visible_author_profile_claim_release_are_idempotent() {
        let (runtime, _rx) = AppRuntime::new().expect("runtime starts without live relays");

        assert_eq!(runtime.claim_visible_author_profile(ALICE), Ok(()));
        assert_eq!(runtime.claim_visible_author_profile(ALICE), Ok(()));
        assert_eq!(runtime.release_visible_author_profile(ALICE), Ok(()));
        assert_eq!(runtime.release_visible_author_profile(ALICE), Ok(()));
    }

    #[test]
    fn note_relation_count_claim_release_are_idempotent() {
        let (runtime, _rx) = AppRuntime::new().expect("runtime starts without live relays");

        assert_eq!(runtime.claim_visible_note_relation_counts(EVENT), Ok(()));
        assert_eq!(runtime.claim_visible_note_relation_counts(EVENT), Ok(()));
        assert_eq!(runtime.release_visible_note_relation_counts(EVENT), Ok(()));
        assert_eq!(runtime.release_visible_note_relation_counts(EVENT), Ok(()));
        assert_eq!(
            runtime.claim_visible_note_relation_counts("bad"),
            Err("event id must be 64 hex characters".to_string())
        );
    }

    #[test]
    fn dispatch_envelope_requires_correlation_id_or_error() {
        assert_eq!(
            parse_dispatch_envelope(&serde_json::json!({"correlation_id": "abc"})),
            Ok("abc".to_string())
        );
        assert_eq!(
            parse_dispatch_envelope(&serde_json::json!({"error": "bad action"})),
            Err("bad action".to_string())
        );
        assert_eq!(
            parse_dispatch_envelope(&serde_json::json!({"ok": true})),
            Err("action dispatch envelope missing correlation_id".to_string())
        );
    }
}
