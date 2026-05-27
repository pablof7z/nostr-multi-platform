//! Shared test harness for `EventStore` tests.
//!
//! Provides `StoreHarness` (wraps a `Box<dyn EventStore>`) and the
//! `for_each_backend!` macro that runs a test body against both `MemEventStore`
//! and (when `lmdb-backend` is enabled) `LmdbEventStore`.
//!
//! See `docs/design/lmdb/tests.md` §1 for the harness specification.

use nmp_core::store::{EventId, EventStore, InsertOutcome, MemEventStore, RawEvent, VerifiedEvent};
use std::sync::atomic::{AtomicU64, Ordering};

// ─── Known test keys ─────────────────────────────────────────────────────────

/// Alice's 32-byte pubkey (all zeros + 0x01 at position 0).
pub const ALICE_PUBKEY: [u8; 32] = {
    let mut k = [0u8; 32];
    k[0] = 0x01;
    k
};

/// Bob's 32-byte pubkey (all zeros + 0x02 at position 0).
pub const BOB_PUBKEY: [u8; 32] = {
    let mut k = [0u8; 32];
    k[0] = 0x02;
    k
};

pub const ALICE_HEX: &str = "0100000000000000000000000000000000000000000000000000000000000000";
pub const BOB_HEX: &str = "0200000000000000000000000000000000000000000000000000000000000000";

// ─── StoreHarness ────────────────────────────────────────────────────────────

/// Wraps a `Box<dyn EventStore>` with helpers for building fixture events.
pub struct StoreHarness {
    pub store: Box<dyn EventStore>,
    next_id: AtomicU64,
}

impl StoreHarness {
    /// Create a harness backed by `MemEventStore`.
    pub fn mem() -> Self {
        Self {
            store: Box::new(MemEventStore::new()),
            next_id: AtomicU64::new(1),
        }
    }

    /// Create a harness backed by `LmdbEventStore` in a temporary directory.
    ///
    /// Only available with `--features lmdb-backend`. Uses a process-unique
    /// path (`pid + counter + nanos`) — `uuid` is intentionally not a dep of
    /// `nmp-testing`, and parallel tests get a fresh path per call.
    #[cfg(feature = "lmdb-backend")]
    pub fn lmdb() -> Self {
        use nmp_core::store::LmdbEventStore;
        use std::sync::atomic::{AtomicU64, Ordering as AOrdering};
        use std::time::{SystemTime, UNIX_EPOCH};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let seq = COUNTER.fetch_add(1, AOrdering::SeqCst);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let tmp =
            std::env::temp_dir().join(format!("nmp-test-{}-{}-{}", std::process::id(), seq, nanos));
        std::fs::create_dir_all(&tmp).expect("create lmdb temp dir");
        Self {
            store: Box::new(LmdbEventStore::open(&tmp).expect("open lmdb store")),
            next_id: AtomicU64::new(1),
        }
    }

    /// Build a minimal valid `RawEvent` with a unique id.
    pub fn make_event(&self, pubkey_hex: &str, kind: u32, created_at: u64) -> RawEvent {
        let seq = self.next_id.fetch_add(1, Ordering::SeqCst);
        let id_hex = format!("{seq:0>64x}");
        RawEvent {
            id: id_hex,
            pubkey: pubkey_hex.to_string(),
            created_at,
            kind,
            tags: vec![],
            content: String::new(),
            sig: "a".repeat(128),
        }
    }

    /// Build a `RawEvent` with a specific id.
    pub fn make_event_with_id(
        &self,
        id_hex: &str,
        pubkey_hex: &str,
        kind: u32,
        created_at: u64,
    ) -> RawEvent {
        RawEvent {
            id: id_hex.to_string(),
            pubkey: pubkey_hex.to_string(),
            created_at,
            kind,
            tags: vec![],
            content: String::new(),
            sig: "a".repeat(128),
        }
    }

    /// Build a `RawEvent` with specific tags.
    pub fn make_event_with_tags(
        &self,
        pubkey_hex: &str,
        kind: u32,
        created_at: u64,
        tags: Vec<Vec<String>>,
    ) -> RawEvent {
        let seq = self.next_id.fetch_add(1, Ordering::SeqCst);
        let id_hex = format!("{seq:0>64x}");
        RawEvent {
            id: id_hex,
            pubkey: pubkey_hex.to_string(),
            created_at,
            kind,
            tags,
            content: String::new(),
            sig: "a".repeat(128),
        }
    }

    /// Insert a pre-built `RawEvent`, panicking on error.
    ///
    /// Uses `VerifiedEvent::from_raw_unchecked` because test events carry
    /// synthetic placeholder signatures.
    pub fn insert_raw(&self, event: RawEvent, source: &str, received_at_ms: u64) -> InsertOutcome {
        let verified = VerifiedEvent::from_raw_unchecked(event);
        self.store
            .insert(verified, &source.to_string(), received_at_ms)
            .expect("insert should not error")
    }

    /// Insert a `RawEvent`, returning the outcome.
    pub fn insert(
        &self,
        pubkey_hex: &str,
        kind: u32,
        created_at: u64,
        source: &str,
    ) -> (EventId, InsertOutcome) {
        let ev = self.make_event(pubkey_hex, kind, created_at);
        let id = ev.id_bytes();
        let outcome = self.insert_raw(ev, source, created_at * 1000);
        (id, outcome)
    }

    /// Assert an event is present in primary storage.
    pub fn assert_present(&self, id: &EventId) {
        assert!(
            self.store
                .get_by_id(id)
                .expect("get_by_id should not error")
                .is_some(),
            "expected event {:?} to be present",
            id
        );
    }

    /// Assert an event is absent from primary storage (tombstoned or never inserted).
    pub fn assert_absent(&self, id: &EventId) {
        assert!(
            self.store
                .get_by_id(id)
                .expect("get_by_id should not error")
                .is_none(),
            "expected event {:?} to be absent",
            id
        );
    }

    /// Assert a tombstone row exists for this event id.
    pub fn assert_tombstoned(&self, id: &EventId) {
        let rows = self
            .store
            .tombstones_for(id)
            .expect("tombstones_for should not error");
        assert!(!rows.is_empty(), "expected tombstone for {:?}", id);
    }

    /// Assert invariants that must hold after every test (§4 of tests.md).
    pub fn assert_invariants(&self) {
        // 1. Every event in the primary store has at least one provenance entry.
        let events_iter = self
            .store
            .scan_by_kind_time(&[], None, None, usize::MAX)
            .expect("scan_by_kind_time should not error");
        for ev_result in events_iter {
            let ev = ev_result.expect("event iteration should not error");
            let id = ev.raw.id_bytes();
            let prov = self
                .store
                .provenance_for(&id)
                .expect("provenance_for should not error");
            assert!(
                !prov.is_empty(),
                "invariant 1 violated: event {:?} has no provenance",
                ev.raw.id
            );
        }

        // 3. Every tombstone's target_id does NOT exist in the primary store.
        let tombs = self
            .store
            .list_tombstones()
            .expect("list_tombstones should not error");
        for tomb_result in tombs {
            let tomb = tomb_result.expect("tombstone iteration should not error");
            let present = self
                .store
                .get_by_id(&tomb.target_id)
                .expect("get_by_id should not error");
            assert!(
                present.is_none(),
                "invariant 3 violated: tombstone target {:?} is still in primary store",
                tomb.target_id
            );
        }
    }
}

/// Helper to convert a hex event id to bytes.
pub fn hex_to_id(hex: &str) -> EventId {
    let mut out = [0u8; 32];
    if hex.len() != 64 {
        return out;
    }
    for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
        if i >= 32 {
            break;
        }
        if let (Some(&hi), Some(&lo)) = (chunk.first(), chunk.get(1)) {
            out[i] = (nibble(hi) << 4) | nibble(lo);
        }
    }
    out
}

fn nibble(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        b'A'..=b'F' => b - b'A' + 10,
        _ => 0,
    }
}

// ─── for_each_backend macro ───────────────────────────────────────────────────

/// Run a test body against both `MemEventStore` and (optionally) `LmdbEventStore`.
///
/// Usage:
/// ```ignore
/// for_each_backend!(my_test_name, |h: &mut StoreHarness| {
///     // ... test body using h
/// });
/// ```
///
/// This expands to two `#[test]` functions: `my_test_name` (mem) and
/// `my_test_name_lmdb` (LMDB, only when `--features lmdb-backend` is enabled).
#[macro_export]
macro_rules! for_each_backend {
    ($name:ident, $body:expr) => {
        #[test]
        fn $name() {
            let mut h = $crate::store_harness::StoreHarness::mem();
            let body: &dyn Fn(&mut $crate::store_harness::StoreHarness) = &$body;
            body(&mut h);
            h.assert_invariants();
        }

        #[cfg(feature = "lmdb-backend")]
        paste::paste! {
            #[test]
            fn [<$name _lmdb>]() {
                let mut h = $crate::store_harness::StoreHarness::lmdb();
                let body: &dyn Fn(&mut $crate::store_harness::StoreHarness) = &$body;
                body(&mut h);
                h.assert_invariants();
            }
        }
    };
}
