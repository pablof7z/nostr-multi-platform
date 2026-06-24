//! ADR-0058 §3 (step 3b) — `nmp_mirror_pull_page` C-ABI tests.
//!
//! Covers the load-bearing boundary contracts: a null app and an unknown cursor
//! return a serialized `Error` (never a panic/null deref), Page and Gap decode
//! as distinct variants, and the entry cap clamps. The Page path drives a REAL
//! ingest through the actor thread (same harness shape as `event_by_id_tests`),
//! so it proves the FFI reads the live kernel store under the documented lock
//! order.

use super::{nmp_mirror_free_bytes, nmp_mirror_pull_page, NmpMirrorBytes};
use crate::{app_ref, nmp_app_free, nmp_app_new, nmp_app_set_update_callback, nmp_app_start};
use nmp_core::actor::{InterestsCommand};
use nmp_core::{PullConsumerId, PullCursorMode, PullCursorSpec, PullLimits, PullScope};
use nmp_core::actor::{ActorCommand};
use nostr::prelude::*;
use std::ffi::c_void;
use std::num::NonZeroUsize;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

static SERIAL: Mutex<()> = Mutex::new(());
static UPDATE_TX: OnceLock<Mutex<Option<Sender<()>>>> = OnceLock::new();

extern "C" fn update_signal_callback(_ctx: *mut c_void, _ptr: *const u8, _len: usize) {
    if let Some(slot) = UPDATE_TX.get() {
        if let Ok(guard) = slot.lock() {
            if let Some(tx) = guard.as_ref() {
                let _ = tx.send(());
            }
        }
    }
}

fn install_update_signal() -> Receiver<()> {
    let (tx, rx) = channel::<()>();
    let slot = UPDATE_TX.get_or_init(|| Mutex::new(None));
    *slot.lock().unwrap() = Some(tx);
    rx
}

fn uninstall_update_signal() {
    if let Some(slot) = UPDATE_TX.get() {
        *slot.lock().unwrap() = None;
    }
}

fn signed_note(content: &str, created_at: u64) -> (String, String) {
    let keys = Keys::generate();
    let event = EventBuilder::text_note(content)
        .custom_created_at(Timestamp::from(created_at))
        .sign_with_keys(&keys)
        .expect("sign note");
    (event.id.to_hex(), event.as_json())
}

fn inject_and_wait(app: *mut crate::NmpApp, id: &str, json: &str, rx: &Receiver<()>) {
    let json_c = std::ffi::CString::new(json).expect("event json");
    assert!(
        crate::nmp_app_inject_signed_event_json(app, json_c.as_ptr()),
        "signed event must inject"
    );
    let app_ref = app_ref(app).expect("app");
    if app_ref.event_by_id(id).is_some() {
        return;
    }
    loop {
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(()) => {
                if app_ref.event_by_id(id).is_some() {
                    return;
                }
            }
            Err(_) => panic!("actor never made the ingested event readable in time"),
        }
    }
}

// ─── Minimal wire-format reader (mirrors pull.rs encoding) ──────────────────

struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }
    fn u8(&mut self) -> u8 {
        let v = self.buf[self.pos];
        self.pos += 1;
        v
    }
    fn u32(&mut self) -> u32 {
        let v = u32::from_le_bytes(self.buf[self.pos..self.pos + 4].try_into().unwrap());
        self.pos += 4;
        v
    }
    fn u64(&mut self) -> u64 {
        let v = u64::from_le_bytes(self.buf[self.pos..self.pos + 8].try_into().unwrap());
        self.pos += 8;
        v
    }
    fn lp(&mut self) -> Vec<u8> {
        let n = self.u32() as usize;
        let v = self.buf[self.pos..self.pos + n].to_vec();
        self.pos += n;
        v
    }
}

#[derive(Debug)]
#[allow(dead_code)] // some fields are decoded for completeness but not asserted
struct DecodedEntry {
    seq: u64,
    op_tag: u8,
    event_id_hex: String,
    has_raw: bool,
}

#[derive(Debug)]
#[allow(dead_code)] // Gap fields are decoded for completeness but not asserted
enum Decoded {
    Page {
        next_after_seq: u64,
        latest_seq: u64,
        has_more: bool,
        entries: Vec<DecodedEntry>,
    },
    Gap {
        requested_after_seq: u64,
        first_available_seq: u64,
    },
    Error(u32),
}

fn decode(bytes: &NmpMirrorBytes) -> Decoded {
    // SAFETY: test reads the buffer the call just produced.
    let slice = unsafe { std::slice::from_raw_parts(bytes.ptr, bytes.len) };
    let mut r = Reader::new(slice);
    match r.u8() {
        0 => {
            let next_after_seq = r.u64();
            let latest_seq = r.u64();
            let has_more = r.u8() == 1;
            let count = r.u32();
            let mut entries = Vec::new();
            for _ in 0..count {
                let seq = r.u64();
                let op_tag = r.u8();
                if op_tag == 1 {
                    let _ = r.lp(); // replaced_id
                } else if op_tag == 2 {
                    let _ = r.lp(); // target_id
                    let _ = r.u8(); // reason
                }
                let event_id_hex = String::from_utf8(r.lp()).unwrap();
                let has_raw = r.u8() == 1;
                if has_raw {
                    let _ = r.lp();
                }
                let _ = r.lp(); // source_relay
                let _ = r.u64(); // received_at_ms
                entries.push(DecodedEntry {
                    seq,
                    op_tag,
                    event_id_hex,
                    has_raw,
                });
            }
            Decoded::Page {
                next_after_seq,
                latest_seq,
                has_more,
                entries,
            }
        }
        1 => Decoded::Gap {
            requested_after_seq: r.u64(),
            first_available_seq: r.u64(),
        },
        2 => Decoded::Error(r.u32()),
        v => panic!("unknown variant tag {v}"),
    }
}

/// Allocate a cursor handle from the app's registry and send OpenPullCursor.
/// Returns the raw cursor id so callers can pass it to `nmp_mirror_pull_page`.
fn register_global_cursor(app: *mut crate::NmpApp, after_seq: u64) -> u64 {
    let app_ref = app_ref(app).expect("app");
    // Allocate the handle under a brief registry write lock — the canonical
    // allocation path (hosts never mint raw cursor ids).
    let handle = {
        let slot = app_ref.pull_cursor_registry_handle();
        let guard = slot.lock().expect("registry slot lock");
        let registry_arc = guard.as_ref().expect("registry not yet published");
        let mut reg = registry_arc.write().expect("registry write lock");
        reg.alloc_handle()
    };
    let cursor_id = handle.id().0;
    let spec = PullCursorSpec {
        consumer_id: PullConsumerId("test-mirror".into()),
        scope: PullScope::GlobalLog,
        mode: PullCursorMode::GapAllowed,
        after_seq,
        limits: PullLimits {
            max_entries: NonZeroUsize::new(256).unwrap(),
            max_scan_entries: NonZeroUsize::new(256).unwrap(),
        },
    };
    app_ref.send_cmd(ActorCommand::Interests(InterestsCommand::OpenPullCursor { handle, spec }));
    cursor_id
}

#[test]
fn null_app_returns_serialized_error_not_panic() {
    let bytes = nmp_mirror_pull_page(std::ptr::null(), 1, 256, 1 << 20);
    match decode(&bytes) {
        Decoded::Error(code) => assert_eq!(code, super::error::NULL_APP),
        other => panic!("expected Error(NULL_APP), got {other:?}"),
    }
    nmp_mirror_free_bytes(bytes);
}

#[test]
fn unknown_cursor_returns_serialized_error() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let _rx = install_update_signal();
    let app = nmp_app_new();
    nmp_app_set_update_callback(app, std::ptr::null_mut(), Some(update_signal_callback));
    nmp_app_start(app, 256, 4);

    // No cursor registered under id 999 → UNKNOWN_CURSOR (registry is published,
    // but holds no row). Poll briefly so the registry slot is published first.
    let mut decoded = None;
    for _ in 0..50 {
        let bytes = nmp_mirror_pull_page(app, 999, 256, 1 << 20);
        let d = decode(&bytes);
        nmp_mirror_free_bytes(bytes);
        if let Decoded::Error(code) = d {
            if code == super::error::UNKNOWN_CURSOR {
                decoded = Some(code);
                break;
            }
            // REGISTRY_UNAVAILABLE before the actor published — retry.
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(decoded, Some(super::error::UNKNOWN_CURSOR));

    nmp_app_free(app);
    uninstall_update_signal();
}

#[test]
fn page_decodes_with_entries_and_cap_clamps() {
    let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let rx = install_update_signal();
    let app = nmp_app_new();
    nmp_app_set_update_callback(app, std::ptr::null_mut(), Some(update_signal_callback));
    nmp_app_start(app, 256, 4);

    // Register first (FIFO before the ingests below), then ingest two events.
    // The registry must be available before alloc_handle (actor publishes it on
    // start). Poll briefly until the slot is populated.
    let cursor_id = {
        let mut cid = None;
        let app_ref = app_ref(app).expect("app");
        for _ in 0..50 {
            let slot = app_ref.pull_cursor_registry_handle();
            let guard = slot.lock().expect("registry slot lock");
            if guard.is_some() {
                drop(guard);
                cid = Some(register_global_cursor(app, 0));
                break;
            }
            drop(guard);
            std::thread::sleep(Duration::from_millis(10));
        }
        cid.expect("registry never published")
    };
    let (id1, json1) = signed_note("pull one", 1_700_100_000);
    let (id2, json2) = signed_note("pull two", 1_700_100_001);
    inject_and_wait(app, &id1, &json1, &rx);
    inject_and_wait(app, &id2, &json2, &rx);

    // Full drain: both entries, has_more=false.
    let bytes = nmp_mirror_pull_page(app, cursor_id, 256, 1 << 20);
    let d = decode(&bytes);
    nmp_mirror_free_bytes(bytes);
    match d {
        Decoded::Page {
            entries,
            has_more,
            latest_seq,
            ..
        } => {
            assert_eq!(entries.len(), 2, "both ingested events delivered");
            assert!(!has_more, "fully drained → has_more=false");
            assert_eq!(latest_seq, 2, "store head at seq 2");
            assert_eq!(entries[0].op_tag, 0, "Inserted");
            assert!(entries[0].has_raw, "Inserted carries raw bytes");
            let ids: Vec<&str> = entries.iter().map(|e| e.event_id_hex.as_str()).collect();
            assert!(ids.contains(&id1.as_str()) && ids.contains(&id2.as_str()));
        }
        other => panic!("expected Page, got {other:?}"),
    }

    // Cap clamp: max_entries=1 yields exactly one entry, has_more=true,
    // next_after_seq=1 (the first row's seq).
    let bytes = nmp_mirror_pull_page(app, cursor_id, 1, 1 << 20);
    let d = decode(&bytes);
    nmp_mirror_free_bytes(bytes);
    match d {
        Decoded::Page {
            entries,
            has_more,
            next_after_seq,
            ..
        } => {
            assert_eq!(entries.len(), 1, "max_entries=1 clamps the page");
            assert!(has_more, "more remains → has_more=true");
            assert_eq!(next_after_seq, 1, "cursor advanced to first row seq");
        }
        other => panic!("expected clamped Page, got {other:?}"),
    }

    nmp_app_free(app);
    uninstall_update_signal();
}

#[test]
fn free_bytes_null_is_no_op() {
    nmp_mirror_free_bytes(NmpMirrorBytes {
        ptr: std::ptr::null_mut(),
        len: 0,
        cap: 0,
    });
}

#[test]
fn gap_and_error_encode_as_distinct_variants() {
    // Direct encoder coverage: Gap and Error are distinct from Page.
    let gap = super::encode_gap(3, 9);
    assert_eq!(gap[0], super::variant::GAP);
    let err = NmpMirrorBytes::error(super::error::STORE_UNAVAILABLE);
    match decode(&err) {
        Decoded::Error(c) => assert_eq!(c, super::error::STORE_UNAVAILABLE),
        other => panic!("expected Error, got {other:?}"),
    }
    nmp_mirror_free_bytes(err);
}

/// Hard cap: a first row whose raw event alone exceeds the byte cap cannot be
/// represented within the promised bound, so `encode_page` returns
/// `RAW_TOO_LARGE` rather than silently overshooting.
#[test]
fn first_row_raw_over_cap_is_hard_error() {
    use nmp_store::{LogOp, PullPage, RawEvent, StoreLogEntry};
    let big_raw = RawEvent {
        id: "aa".repeat(32),
        pubkey: "bb".repeat(32),
        created_at: 1_700_000_000,
        kind: 1,
        tags: vec![],
        content: "x".repeat(5_000),
        sig: "cc".repeat(64),
    };
    let entry = StoreLogEntry {
        seq: 1,
        op: LogOp::Inserted,
        event_id: [0u8; 32],
        raw_event: Some(big_raw),
        source_relay: None,
        received_at_ms: 0,
    };
    let page = PullPage {
        entries: vec![entry],
        next_after_seq: 1,
        latest_seq: 1,
        has_more: false,
    };
    // Cap of 100 bytes is far below the ~5 KiB serialized raw ⇒ hard error.
    assert_eq!(
        super::encode_page(page, 100),
        Err(super::error::RAW_TOO_LARGE)
    );
}
