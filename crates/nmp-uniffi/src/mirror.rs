//! ADR-0058 §3 mirror pull-page surface — M14-C7 UniFFI migration.
//!
//! Typed `#[uniffi::export] impl NmpApp` mirror pull-page method. The
//! COLLAPSE benefit: the byte payload rides UniFFI's `Vec<u8>` lifetime with no
//! separate freer, and the cursor-metadata fields (`next_after_seq`,
//! `latest_seq`, `has_more`) are typed values rather than embedded bytes.
//!
//! The old public C doorway is intentionally not preserved. UniFFI manages the
//! `Vec<u8>` lifetime in `MirrorPullResult::Page.bytes`, so the host never
//! touches a raw allocator pointer and needs no explicit free call.
//!
//! ## Result variant mapping
//!
//! The runtime binary format (variant 0 = Page, 1 = Gap, 2 = Error) maps to
//! the typed `MirrorPullResult` enum. For `Page`, the typed metadata fields
//! are extracted and the entry-payload bytes are returned as `Vec<u8>` whose
//! format is the ADR-0058 entry section (u32_LE count + entries).
//!
//! ## Implementation note
//!
//! `mirror_pull_page` calls `self.inner.mirror_pull_page_raw_bytes(...)` — the
//! shared runtime method, then lifts the binary result to a typed enum. No C
//! symbols are called; no logic is duplicated (encoding lives in
//! `nmp_native_runtime::app_mirror`).

use crate::NmpApp;
use nmp_native_runtime::app_mirror;

// ── Typed result enum ─────────────────────────────────────────────────────────

/// Typed result of a `mirror_pull_page` call.
///
/// Typed variants for the mirror pull result. UniFFI owns all `Vec<u8>`
/// payloads, so no explicit free call is needed.
#[derive(uniffi::Enum, Debug, Clone)]
pub enum MirrorPullResult {
    /// A page of log entries was returned.
    ///
    /// - `next_after_seq` — advance the cursor to this value for the next call.
    /// - `latest_seq`     — the store's current head sequence at the time of
    ///   the read; use with `next_after_seq` to detect lag.
    /// - `has_more`       — `true` when the store head is ahead of
    ///   `next_after_seq`; call again to drain.
    /// - `bytes`          — serialized entry section: `u32_LE entry_count`
    ///   followed by `entry_count × entry` in the ADR-0058 §3 wire format.
    ///   Identical to bytes `[18..]` of the runtime page payload.
    Page {
        next_after_seq: u64,
        latest_seq: u64,
        has_more: bool,
        bytes: Vec<u8>,
    },
    /// The requested `after_seq` fell before the store's earliest available
    /// entry; the host must decide how to handle the gap.
    Gap {
        requested_after_seq: u64,
        first_available_seq: u64,
    },
    /// An error occurred before the pull could proceed.
    ///
    /// `code` values match the runtime mirror error constants:
    /// 2 = REGISTRY_UNAVAILABLE (pre-start), 3 = UNKNOWN_CURSOR,
    /// 4 = STORE_UNAVAILABLE, 5 = UNSUPPORTED_SCOPE, 6 = STORE_ERROR,
    /// 7 = INVALID_LIMITS, 8 = PANIC, 9 = RAW_TOO_LARGE.
    Error { code: u32 },
}

// ── Wire-format byte offsets (Page header) ────────────────────────────────────
//
// Layout produced by `nmp_native_runtime::app_mirror::encode_page`:
//   byte  0    : variant (0 = PAGE)
//   bytes 1-8  : next_after_seq (u64 LE)
//   bytes 9-16 : latest_seq (u64 LE)
//   byte  17   : has_more (u8, 0 or 1)
//   bytes 18.. : entry section (u32_LE entry_count + entries)

const PAGE_HDR: usize = 1 + 8 + 8 + 1; // 18 bytes
const GAP_LEN: usize = 1 + 8 + 8; // 17 bytes
const ERROR_LEN: usize = 1 + 4; // 5 bytes

// ── NmpApp::mirror_pull_page ──────────────────────────────────────────────────

#[uniffi::export]
impl NmpApp {
    /// ADR-0058 §3 — synchronously drain one page of the kernel ingest log.
    ///
    /// Returns a typed [`MirrorPullResult`]; UniFFI owns the returned `Vec<u8>`.
    ///
    /// - `cursor_id`           — raw u64 id from `PullCursorRegistry`.
    /// - `max_entries`         — clamped to `[1, 512]`; further bounded by
    ///   the cursor's registered `limits.max_entries`.
    /// - `max_total_raw_bytes` — cumulative raw-event byte budget; capped at
    ///   4 MiB. At least one entry is always delivered so the cursor advances.
    ///
    /// D6: never throws; every error surface as `MirrorPullResult::Error`.
    pub fn mirror_pull_page(
        &self,
        cursor_id: u64,
        max_entries: u32,
        max_total_raw_bytes: u32,
    ) -> MirrorPullResult {
        let raw = self.inner.mirror_pull_page_raw_bytes(
            cursor_id,
            max_entries,
            max_total_raw_bytes as usize,
        );
        parse_raw(raw)
    }
}

/// Lift the binary `raw` payload to a typed [`MirrorPullResult`].
///
/// Reads the variant byte (offset 0), then extracts typed fields from the
/// known offsets. An unrecognized or truncated payload maps to
/// `Error { code: PANIC }` (D6 fail-closed).
fn parse_raw(raw: Vec<u8>) -> MirrorPullResult {
    match raw.first().copied() {
        Some(0) if raw.len() >= PAGE_HDR => {
            // SAFETY of index slices: length checked above (>= 18 bytes).
            let next_after_seq = u64::from_le_bytes(raw[1..9].try_into().unwrap_or([0; 8]));
            let latest_seq = u64::from_le_bytes(raw[9..17].try_into().unwrap_or([0; 8]));
            let has_more = raw[17] != 0;
            // bytes[18..] = u32_LE entry_count + entries (the full entry section).
            let bytes = raw[PAGE_HDR..].to_vec();
            MirrorPullResult::Page {
                next_after_seq,
                latest_seq,
                has_more,
                bytes,
            }
        }
        Some(1) if raw.len() >= GAP_LEN => {
            let requested_after_seq = u64::from_le_bytes(raw[1..9].try_into().unwrap_or([0; 8]));
            let first_available_seq = u64::from_le_bytes(raw[9..17].try_into().unwrap_or([0; 8]));
            MirrorPullResult::Gap {
                requested_after_seq,
                first_available_seq,
            }
        }
        Some(2) if raw.len() >= ERROR_LEN => {
            let code = u32::from_le_bytes(raw[1..5].try_into().unwrap_or([0; 4]));
            MirrorPullResult::Error { code }
        }
        // Malformed response (should never happen from mirror_pull_page_raw_bytes).
        _ => MirrorPullResult::Error {
            code: app_mirror::error::PANIC,
        },
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    use std::num::NonZeroUsize;
    use std::sync::mpsc::{channel, Receiver, Sender};
    use std::sync::Mutex;
    use std::time::Duration;

    use nmp_core::actor::{ActorCommand, InterestsCommand};
    use nmp_core::{PullConsumerId, PullCursorMode, PullCursorSpec, PullLimits, PullScope};
    use nostr::prelude::*;

    // ── Test harness ─────────────────────────────────────────────────────────

    static SERIAL: Mutex<()> = Mutex::new(());

    /// `UpdateSink` that forwards every frame to a shared channel.
    ///
    /// Unlike a one-shot sink, this fires on every frame so `inject_and_wait`
    /// can block on frames that arrive AFTER the passive pre-start snapshot.
    struct RepeatingSignalSink {
        tx: std::sync::Arc<Mutex<Sender<()>>>,
    }

    impl crate::UpdateSink for RepeatingSignalSink {
        fn on_update(&self, _frame: Vec<u8>) {
            if let Ok(guard) = self.tx.lock() {
                let _ = guard.send(());
            }
        }
    }

    fn new_started_app() -> (std::sync::Arc<crate::NmpApp>, Receiver<()>) {
        let (tx, rx) = channel::<()>();
        let tx = std::sync::Arc::new(Mutex::new(tx));
        let app = crate::NmpApp::new();
        app.set_update_sink(Some(Box::new(RepeatingSignalSink { tx })));
        app.start(100, 6);
        (app, rx)
    }

    fn signed_note(content: &str, created_at: u64) -> (String, String) {
        let keys = Keys::generate();
        let event = EventBuilder::text_note(content)
            .custom_created_at(Timestamp::from(created_at))
            .sign_with_keys(&keys)
            .expect("sign note");
        (event.id.to_hex(), event.as_json())
    }

    fn inject_and_wait(app: &crate::NmpApp, id: &str, json: &str, rx: &Receiver<()>) {
        assert!(
            app.inner.inject_signed_event_json_for_test(json),
            "event must inject"
        );
        if app.inner.event_by_id(id).is_some() {
            return;
        }
        loop {
            match rx.recv_timeout(Duration::from_secs(5)) {
                Ok(()) => {
                    if app.inner.event_by_id(id).is_some() {
                        return;
                    }
                }
                Err(_) => panic!("actor never made event {id} readable"),
            }
        }
    }

    fn register_global_cursor(app: &crate::NmpApp, after_seq: u64) -> u64 {
        let slot = app.inner.pull_cursor_registry_handle();
        let handle = {
            let guard = slot.lock().expect("registry slot lock");
            let registry_arc = guard.as_ref().expect("registry not published");
            let mut reg = registry_arc.write().expect("registry write lock");
            reg.alloc_handle()
        };
        let cursor_id = handle.id().0;
        let spec = PullCursorSpec {
            consumer_id: PullConsumerId("uniffi-test".into()),
            scope: PullScope::GlobalLog,
            mode: PullCursorMode::GapAllowed,
            after_seq,
            limits: PullLimits {
                max_entries: NonZeroUsize::new(256).unwrap(),
                max_scan_entries: NonZeroUsize::new(256).unwrap(),
            },
        };
        app.inner
            .send_cmd(ActorCommand::Interests(InterestsCommand::OpenPullCursor {
                handle,
                spec,
            }));
        cursor_id
    }

    fn wait_registry_ready(app: &crate::NmpApp) {
        for _ in 0..50 {
            let slot = app.inner.pull_cursor_registry_handle();
            if slot.lock().map_or(false, |g| g.is_some()) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("registry slot never published");
    }

    // ── Parity tests ─────────────────────────────────────────────────────────

    /// D6: unknown cursor returns `Error { code: UNKNOWN_CURSOR }`, never panics.
    ///
    /// Parity with the shared runtime unknown-cursor encoding.
    #[test]
    fn parity_unknown_cursor_returns_error() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let (app, _rx) = new_started_app();
        wait_registry_ready(&app);

        let mut got_unknown = false;
        for _ in 0..50 {
            match app.mirror_pull_page(999, 256, 1 << 20) {
                MirrorPullResult::Error { code } if code == app_mirror::error::UNKNOWN_CURSOR => {
                    got_unknown = true;
                    break;
                }
                // REGISTRY_UNAVAILABLE before the actor has published — retry.
                MirrorPullResult::Error { .. } => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                other => panic!("expected Error(UNKNOWN_CURSOR), got {other:?}"),
            }
        }
        assert!(got_unknown, "never received UNKNOWN_CURSOR");
        app.shutdown();
    }

    /// Parity: `Page` variant carries typed metadata and binary entry bytes
    /// that decode equivalently to the shared runtime encoding.
    ///
    /// Parity with the shared runtime page encoding and cap clamps.
    #[test]
    fn parity_page_typed_metadata_and_entry_bytes_round_trip() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let (app, rx) = new_started_app();
        wait_registry_ready(&app);

        let cursor_id = register_global_cursor(&app, 0);
        let (id1, json1) = signed_note("uniffi-mirror-one", 1_700_200_000);
        let (id2, json2) = signed_note("uniffi-mirror-two", 1_700_200_001);
        inject_and_wait(&app, &id1, &json1, &rx);
        inject_and_wait(&app, &id2, &json2, &rx);

        // Full drain: both entries delivered, has_more=false.
        let result = app.mirror_pull_page(cursor_id, 256, 1 << 20);
        match result {
            MirrorPullResult::Page {
                next_after_seq,
                latest_seq,
                has_more,
                ref bytes,
            } => {
                assert_eq!(latest_seq, 2, "store head at seq 2");
                assert!(!has_more, "fully drained → has_more=false");
                assert_eq!(next_after_seq, 2, "cursor advanced to seq 2");

                // bytes = u32_LE entry_count + entries (same binary as raw [18..]).
                assert!(
                    bytes.len() >= 4,
                    "entry section must have at least the count"
                );
                let entry_count = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
                assert_eq!(entry_count, 2, "both events in entry section");
            }
            other => panic!("expected Page, got {other:?}"),
        }

        // Cap clamp: max_entries=1 → one entry, has_more=true, next_after_seq=1.
        let result = app.mirror_pull_page(cursor_id, 1, 1 << 20);
        match result {
            MirrorPullResult::Page {
                next_after_seq,
                has_more,
                ref bytes,
                ..
            } => {
                assert_eq!(next_after_seq, 1, "cursor advanced to first seq");
                assert!(has_more, "more remains → has_more=true");
                let entry_count = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
                assert_eq!(entry_count, 1, "one entry after cap clamp");
            }
            other => panic!("expected clamped Page, got {other:?}"),
        }

        app.shutdown();
    }

    /// Parity: bytes round-trips correctly — the entry bytes returned by
    /// `mirror_pull_page` match the shared runtime payload at offset [18..].
    #[test]
    fn parity_bytes_match_runtime_entry_section() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let (app, rx) = new_started_app();
        wait_registry_ready(&app);

        let cursor_id = register_global_cursor(&app, 0);
        let (id1, json1) = signed_note("bytes-parity", 1_700_300_000);
        inject_and_wait(&app, &id1, &json1, &rx);

        // Obtain the runtime binary payload for the same cursor/params.
        let raw_bytes = app
            .inner
            .mirror_pull_page_raw_bytes(cursor_id, 256, 1 << 20);

        // Parse the runtime binary to extract the entry section (offset 18+).
        assert_eq!(raw_bytes[0], nmp_native_runtime::app_mirror::variant::PAGE);
        let runtime_entry_section = &raw_bytes[PAGE_HDR..];

        // Now call the UniFFI surface (fresh cursor same position since we
        // just consumed the only event; re-register at seq=0 to replay).
        let cursor_id2 = register_global_cursor(&app, 0);
        // Wait briefly for the new cursor to be registered.
        std::thread::sleep(Duration::from_millis(50));
        let result = app.mirror_pull_page(cursor_id2, 256, 1 << 20);
        match result {
            MirrorPullResult::Page { bytes, .. } => {
                assert_eq!(
                    bytes.as_slice(),
                    runtime_entry_section,
                    "UniFFI bytes must exactly match the runtime entry section"
                );
            }
            other => panic!("expected Page, got {other:?}"),
        }

        app.shutdown();
    }

    /// `parse_raw` fallback: a malformed raw payload returns `Error { PANIC }`.
    #[test]
    fn parse_raw_malformed_returns_panic_error() {
        assert!(matches!(
            parse_raw(vec![]),
            MirrorPullResult::Error { code } if code == app_mirror::error::PANIC
        ));
        // Truncated Page (only variant byte, no metadata).
        assert!(matches!(
            parse_raw(vec![0]),
            MirrorPullResult::Error { code } if code == app_mirror::error::PANIC
        ));
        // Truncated Gap (only variant byte).
        assert!(matches!(
            parse_raw(vec![1]),
            MirrorPullResult::Error { code } if code == app_mirror::error::PANIC
        ));
        // Unknown variant byte.
        assert!(matches!(
            parse_raw(vec![99]),
            MirrorPullResult::Error { code } if code == app_mirror::error::PANIC
        ));
    }

    /// `parse_raw` correctly lifts a well-formed Error payload.
    #[test]
    fn parse_raw_error_variant_decodes_code() {
        let error_bytes = app_mirror::error_bytes(app_mirror::error::UNKNOWN_CURSOR);
        match parse_raw(error_bytes) {
            MirrorPullResult::Error { code } => {
                assert_eq!(code, app_mirror::error::UNKNOWN_CURSOR);
            }
            other => panic!("expected Error, got {other:?}"),
        }
    }

    /// `parse_raw` correctly lifts a well-formed Gap payload.
    #[test]
    fn parse_raw_gap_variant_decodes_seqs() {
        let gap_bytes = app_mirror::encode_gap(42, 100);
        match parse_raw(gap_bytes) {
            MirrorPullResult::Gap {
                requested_after_seq,
                first_available_seq,
            } => {
                assert_eq!(requested_after_seq, 42);
                assert_eq!(first_available_seq, 100);
            }
            other => panic!("expected Gap, got {other:?}"),
        }
    }
}
