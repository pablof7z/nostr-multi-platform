//! No-raw-tap-reintroduction — guards against re-introducing the deleted raw
//! event tap escape hatch, the public filterless accepted-event observer lane,
//! AND the #1552-deleted native push C-ABI sink.
//!
//! ## What was deleted
//!
//! ### 1. Raw event tap
//! The raw event tap (`RawEventObserver`, `register_raw_event_observer`, and
//! friends) was a first-generation escape hatch that forwarded verbatim NIP-01
//! signed events to external stores. It has been replaced by two purpose-built
//! seams:
//!
//! - `IngestParser` (slot-keyed, cache-replay-aware kind-level parser)
//! - `ExternalEventSinkPolicy` / `ExternalEventSinkDispatcher` (in-process
//!   relay-forwarding policy — **NOT** an external consumer API)
//!
//! ### 2. Native push C-ABI sink (#1552)
//! The speculative batched `ExternalEventSink` C-ABI (register/ack/store-resync)
//! briefly stood in for the raw tap and has also been removed. It had the same
//! fundamental backpressure problems: C-ABI register/ack + retain-until-ack
//! cursor + `created_at` store-resync watermark.
//!
//! External per-event consumption (e.g. the `hl` nostrdb mirror) uses the pull
//! cursor (ADR-0058 §8 step 5): register a `GlobalLog` cursor in
//! `Protected { max_lag_entries }` mode → receive `nmp.pull.wake` →
//! call UniFFI `NmpApp::mirror_pull_page` → apply the page → persist `after_seq` →
//! `AdvancePullCursor`. See `docs/architecture/external-consumers.md`.
//!
//! ### 3. Public filterless accepted-event observers (#2089)
//! The public `KernelEventObserver` / `register_live_event_tap` lane let app
//! and product code subscribe to every accepted event with no declared shape.
//! Production read models now use `ObservedProjectionRegistrar` with a concrete
//! `ObservedProjection` declaration: shape, scope, owner, replay, then scoped
//! future delivery. Blanket all-event fan-out is kernel-internal only.
//!
//! Because scoped projections and the pull cursor exist, none of these old
//! shapes may be re-introduced —
//! not even as a "compatibility shim", an "interim bridge", or a "legacy mode".
//!
//! ## What this catches
//!
//! Any non-comment line that contains one of the following tokens is flagged:
//!
//! ### Deleted raw event tap symbols
//! - `RawEventObserver`
//! - `RawEventObserverFn`
//! - `RawEventObserverId`
//! - `RawEventObserverSlot`
//! - `notify_raw_observers`
//! - `raw_observers_idle_for_kind`
//! - `new_raw_event_observer_slot`
//! - `register_rust_raw_observer`
//! - `register_c_raw_observer`
//! - `unregister_raw_observer`
//! - `register_raw_event_observer` (the trait method + FFI entry point)
//! - `unregister_raw_event_observer` (the trait method + FFI entry point)
//! - `nmp_app_register_raw_event_observer`
//! - `nmp_app_unregister_raw_event_observer`
//! - `NmpRawEventObserverCallback`
//! - `raw_event_tap` (the FFI module name)
//!
//! ### Deleted #1552 native push sink symbols
//! - `nmp_app_register_event_sink`  (C-ABI register for the deleted native sink)
//! - `nmp_app_ack_event_sink_batch` (C-ABI ack for the deleted native sink)
//! - `retain_until_ack`             (retain-until-ack cursor — deleted pattern)
//! - `native_sink_cursor`           (native push sink position tracker)
//! - `NativeEventSinkCallback`      (C-ABI callback type for the deleted sink)
//! - `event_sink_watermark`         (created_at resync watermark for the deleted sink)
//!
//! ### Deleted public filterless observer symbols
//! - `KernelEventObserver`
//! - `LiveEventTapRegistrar`
//! - `register_live_event_tap`
//! - `register_event_observer`
//! - `unregister_event_observer`
//! - `nmp_app_register_event_observer`
//! - `nmp_app_unregister_event_observer`
//! - `NmpEventObserverCallback`
//! - `ObservedProjectionCallbackFn`
//!
//! ## Scope
//!
//! Workspace-wide: production Rust source in `crates/*/src/` and `apps/*/src/`.
//! Test-only files and `#[cfg(test)]` bodies are exempt. The doctrine-lint
//! binary itself is also exempt (this rule file contains the banned tokens as
//! string constants).
//!
//! ## Allowed exemptions
//!
//! - Comment lines.
//! - `#[cfg(test)]` bodies (`sl.in_test_cfg`).
//! - Test-only files (`d6_test_file`).
//! - Per-line `// doctrine-allow: no_raw_tap — reason` opt-out (reason
//!   REQUIRED; a bare allow without justification is rejected).
//!
//! ## Canonical replacements
//!
//! | Old symbol                            | Replacement                                  |
//! |---------------------------------------|----------------------------------------------|
//! | `register_raw_event_observer`         | `register_ingest_parser` (kind-level)        |
//! | `RawEventObserver` callback           | `ExternalEventSinkPolicy` + dispatcher       |
//! | `raw_event_tap` FFI module            | `external_event_sink` substrate seam         |
//! | `nmp_app_register_event_sink` (C-ABI) | `NmpApp::mirror_pull_page` + `GlobalLog` cursor |
//! | `retain_until_ack` cursor             | pull cursor `Protected { max_lag_entries }`  |
//! | `event_sink_watermark` resync         | `after_seq` persisted by pull consumer       |
//! | `register_live_event_tap`             | `open_observed_projection` with a shape      |

pub const ID: &str = "no_raw_tap";

/// Tokens whose presence in non-comment, non-test production code is a
/// regression. Ordered longest-first so a later prefix match cannot shadow
/// a longer one (though in practice all these are distinct identifiers).
///
/// The first group covers the original raw event tap; the second group covers
/// the #1552-deleted native push C-ABI sink.
const BANNED_TOKENS: &[&str] = &[
    // ── raw event tap (original deletion) ────────────────────────────────────
    "nmp_app_register_raw_event_observer",
    "nmp_app_unregister_raw_event_observer",
    "NmpRawEventObserverCallback",
    "register_raw_event_observer",
    "unregister_raw_event_observer",
    "RawEventObserverSlot",
    "RawEventObserverFn",
    "RawEventObserverId",
    "RawEventObserver",
    "notify_raw_observers",
    "raw_observers_idle_for_kind",
    "new_raw_event_observer_slot",
    "register_rust_raw_observer",
    "register_c_raw_observer",
    "unregister_raw_observer",
    "raw_event_tap",
    // ── #1552 native push C-ABI sink (deleted) ────────────────────────────────
    "nmp_app_register_event_sink", // C-ABI register for the deleted native sink
    "nmp_app_ack_event_sink_batch", // C-ABI ack for the deleted native sink
    "retain_until_ack",            // retain-until-ack cursor — deleted pattern
    "native_sink_cursor",          // native push sink position tracker
    "NativeEventSinkCallback",     // C-ABI callback type for the deleted sink
    "event_sink_watermark",        // created_at resync watermark for the deleted sink
    // ── #2089 public filterless accepted-event observers (deleted) ───────────
    "nmp_app_unregister_event_observer",
    "nmp_app_register_event_observer",
    "NmpEventObserverCallback",
    "ObservedProjectionCallbackFn",
    "unregister_event_observer",
    "register_live_event_tap",
    "register_event_observer",
    "LiveEventTapRegistrar",
    "KernelEventObserver",
];

/// Per-line check. Returns `(col, message, suggested)` tuples for each hit.
///
/// Three layers:
/// 1. **Named tokens** — the exact deleted symbols (`RawEventObserver`, …, and
///    the #1552 native-sink symbols) may never reappear.
/// 2. **The raw-tap CLASS** — any *new* below-seam per-event ingest-observer
///    callback shaped as an `extern "C" fn(*mut c_void, *const c_char)`, declared
///    outside the sanctioned `external_event_sink` module, is banned.
/// 3. **The native-sink CLASS** — any *new* below-seam C-ABI event-sink
///    register or ack function outside the sanctioned sink module, with a
///    native-sink intent token, is banned.
///
/// `in_sink_module` is `true` only for files under the bounded
/// `external_event_sink` seam, where such callbacks legitimately live.
pub fn check(
    line: &str,
    is_comment: bool,
    in_test_cfg: bool,
    in_sink_module: bool,
) -> Vec<(usize, String, String)> {
    if is_comment || in_test_cfg {
        return Vec::new();
    }
    let mut hits = Vec::new();
    for token in BANNED_TOKENS {
        if let Some(pos) = line.find(token) {
            hits.push((
                pos + 1, // 1-indexed columns for clippy compatibility
                format!(
                    "`{}` re-introduces a deleted external-event-delivery escape hatch \
                     (#1552 native push sink or raw event tap) — \
                     external per-event mirrors must use the pull cursor \
                     (`NmpApp::mirror_pull_page` + `GlobalLog` cursor, ADR-0058); \
                     in-process relay forwarding uses `ExternalEventSinkPolicy`; \
                     kind-level parsing uses `register_ingest_parser`",
                    token
                ),
                "for an external store mirror: register a `GlobalLog` cursor in \
                 `Protected {{ max_lag_entries }}` mode, receive `nmp.pull.wake`, \
                 call `NmpApp::mirror_pull_page`, apply the page, persist `after_seq`, \
                 then `AdvancePullCursor` (ADR-0058, docs/architecture/external-consumers.md); \
                 for in-process relay forwarding: `ExternalEventSinkPolicy`; \
                 for kind-level parsing: `register_ingest_parser`"
                    .to_string(),
            ));
            // Only report the first hit per line — avoids duplicate findings
            // when a line contains e.g. both a type and a method call.
            return hits;
        }
    }
    // Class checks: catch renamed reincarnations of either banned shape,
    // outside the sanctioned external_event_sink module.
    if !in_sink_module {
        if let Some(pos) = raw_tap_class_violation_col(line) {
            hits.push((
                pos + 1,
                "below-seam per-event ingest-observer callback (an \
                 `extern \"C\" fn(*mut c_void, *const c_char)` raw/signed-event \
                 ingest tap) registered outside the `external_event_sink` \
                 module re-introduces the raw-event-tap CLASS"
                    .to_string(),
                "for external mirrors use the pull cursor (ADR-0058); \
                 for in-process relay forwarding route through \
                 `ExternalEventSinkPolicy` / `ExternalEventSinkDispatcher` in \
                 `substrate/external_event_sink/`"
                    .to_string(),
            ));
        } else if let Some(pos) = native_sink_class_violation_col(line) {
            hits.push((
                pos + 1,
                "C-ABI event-sink register/ack outside the `external_event_sink` \
                 module re-introduces the #1552-deleted native push sink CLASS — \
                 the native push sink (register/ack/retain-until-ack) is permanently \
                 replaced by the pull cursor (ADR-0058)"
                    .to_string(),
                "for external mirrors use UniFFI `NmpApp::mirror_pull_page` + `GlobalLog` cursor \
                 in `Protected {{ max_lag_entries }}` mode (ADR-0058, \
                 docs/architecture/external-consumers.md)"
                    .to_string(),
            ));
        }
    }
    hits
}

/// Detect the raw-tap CLASS: an `extern "C" fn(*mut c_void, *const c_char)`
/// used as a raw/signed-event *ingest observer/tap*. Requires BOTH the C-ABI
/// per-event callback signature AND an observer/tap intent token on the same
/// line, so an unrelated `extern "C"` callback is not a false positive.
fn raw_tap_class_violation_col(line: &str) -> Option<usize> {
    let normalized: String = line.split_whitespace().collect::<Vec<_>>().join(" ");
    let has_c_per_event_cb = normalized.contains("extern \"C\"")
        && normalized.contains("fn(")
        && normalized.contains("*mut c_void")
        && normalized.contains("*const c_char");
    if !has_c_per_event_cb {
        return None;
    }
    // Intent tokens that mark this as a raw/signed per-EVENT ingest tap.
    const INTENT: &[&str] = &[
        "raw_event_observer",
        "raweventobserver",
        "event_observer_callback",
        "raweventcallback",
        "eventtapcallback",
        "ingest_observer",
        "event_tap",
        "raw_tap",
        // Native-sink register/ack intent (catches renamed reincarnation of #1552)
        "native_sink",
        "event_sink_register",
        "sink_ack",
        "ack_event_sink",
    ];
    let lower = line.to_ascii_lowercase();
    if INTENT.iter().any(|t| lower.contains(t)) {
        return line.find("extern").or(Some(0));
    }
    None
}

/// Detect the native-push-sink CLASS: a C-ABI `extern "C" fn` declaration
/// outside the sanctioned sink module that carries a native-sink register or
/// ack intent token. Catches renamed reincarnations of the #1552 sink that
/// do NOT use the literal `*mut c_void` / `*const c_char` signature.
fn native_sink_class_violation_col(line: &str) -> Option<usize> {
    let normalized: String = line.split_whitespace().collect::<Vec<_>>().join(" ");
    if !normalized.contains("extern \"C\"") || !normalized.contains("fn") {
        return None;
    }
    // Intent tokens that specifically mark a native push sink or ack callback
    // outside the `external_event_sink` module.
    const NATIVE_SINK_INTENT: &[&str] = &[
        "register_event_sink",
        "ack_event_sink",
        "event_sink_ack",
        "native_event_sink",
        "push_sink_register",
        "sink_batch_ack",
        "retain_until_ack",
    ];
    let lower = line.to_ascii_lowercase();
    if NATIVE_SINK_INTENT.iter().any(|t| lower.contains(t)) {
        return line.find("extern").or(Some(0));
    }
    None
}

/// True iff the rule should scan `path`.
///
/// Scope: workspace production Rust source under BOTH `crates/` and `apps/`
/// (an app could otherwise re-introduce a below-seam tap). Excludes:
/// - `nmp-testing` crate (test infrastructure and fixture host)
/// - doctrine-lint binary source (contains the banned tokens as string
///   constants — scanning itself would produce meta-false-positives)
pub fn file_in_scope(path: &std::path::Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    // Must be under crates/ or apps/ in the workspace.
    let under_crates = s.contains("/crates/") || s.starts_with("crates/");
    let under_apps = s.contains("/apps/") || s.starts_with("apps/");
    if !under_crates && !under_apps {
        return false;
    }
    // Exempt nmp-testing (test infra + fixtures).
    if s.contains("/crates/nmp-testing/") || s.starts_with("crates/nmp-testing/") {
        return false;
    }
    // Exempt doctrine-lint binary source (contains the banned strings as
    // string constants; scanning would produce meta-false-positives).
    if s.contains("/doctrine-lint/") || s.starts_with("doctrine-lint/") {
        return false;
    }
    true
}

/// True iff `path` is the bounded `external_event_sink` seam, where an
/// `extern "C" fn(*mut c_void, *const c_char)` batch callback legitimately
/// lives. Used to exempt the sanctioned module from the CLASS checks (the named
/// banned tokens still apply everywhere).
pub fn in_sink_module(path: &std::path::Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    s.contains("/external_event_sink/")
        || s.ends_with("/external_event_sink.rs")
        || s.ends_with("external_event_sink.rs")
}
