//! No-raw-tap-reintroduction — guards against re-introducing the deleted raw
//! event tap escape hatch.
//!
//! The raw event tap (`RawEventObserver`, `register_raw_event_observer`, and
//! friends) was a first-generation escape hatch that forwarded verbatim NIP-01
//! signed events to external stores. It has been fully replaced by two
//! purpose-built seams:
//!
//! - `IngestParser` (slot-keyed, cache-replay-aware kind-level parser)
//! - `ExternalEventSinkPolicy` / `ExternalEventSinkDispatcher` (post-ingest
//!   fan-out to external storage with full provenance)
//!
//! Because both replacements exist, the old tap symbols must NEVER be
//! re-introduced — not even as a "compatibility shim", an "interim
//! bridge", or a "legacy mode".
//!
//! ## What this catches
//!
//! Any non-comment line that contains one of the following tokens is flagged:
//!
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
//! ## Scope
//!
//! Workspace-wide: production Rust source in `crates/*/src/`. Test-only files
//! and `#[cfg(test)]` bodies are exempt (test stubs / migration tests may
//! reference the old names as negative-example strings). The doctrine-lint
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
//! | Old symbol                         | Replacement                          |
//! |------------------------------------|--------------------------------------|
//! | `register_raw_event_observer`      | `register_ingest_parser` (kind-level)|
//! | `RawEventObserver` callback        | `ExternalEventSinkPolicy` + dispatcher|
//! | `raw_event_tap` FFI module         | `external_event_sink` substrate seam |

pub const ID: &str = "no_raw_tap";

/// Tokens whose presence in non-comment, non-test production code is a
/// regression. Ordered longest-first so a later prefix match cannot shadow
/// a longer one (though in practice all these are distinct identifiers).
const BANNED_TOKENS: &[&str] = &[
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
];

/// Per-line check. Returns `(col, message, suggested)` tuples for each hit.
///
/// Two layers:
/// 1. **Named tokens** — the exact deleted symbols (`RawEventObserver`, …) may
///    never reappear.
/// 2. **The CLASS** — any *new* below-seam per-event ingest-observer callback,
///    shaped as an `extern "C" fn(*mut c_void, *const c_char)` registered as a
///    raw/signed-event ingest tap, is banned OUTSIDE the sanctioned
///    `external_event_sink` module. This stops a renamed reincarnation from
///    slipping past the literal token list. `in_sink_module` is `true` only for
///    files under the bounded `external_event_sink` seam (substrate module +
///    FFI surface), where such a callback legitimately lives.
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
                    "`{}` re-introduces the deleted raw event tap escape hatch — \
                     use `register_ingest_parser` for kind-level parsing or \
                     `ExternalEventSinkPolicy` for post-ingest fan-out instead",
                    token
                ),
                "replace with `register_ingest_parser` (cache-replay-aware, \
                 slot-keyed) or `ExternalEventSinkPolicy` / \
                 `ExternalEventSinkDispatcher` (post-ingest external-store fan-out)"
                    .to_string(),
            ));
            // Only report the first hit per line — avoids duplicate findings
            // when a line contains e.g. both a type and a method call.
            return hits;
        }
    }
    // Class check: a below-seam per-event ingest-observer callback shaped like
    // the old raw tap, declared outside the sanctioned sink module.
    if !in_sink_module {
        if let Some(pos) = class_violation_col(line) {
            hits.push((
                pos + 1,
                "below-seam per-event ingest-observer callback (an \
                 `extern \"C\" fn(*mut c_void, *const c_char)` raw/signed-event \
                 ingest tap) registered outside the `external_event_sink` \
                 module re-introduces the raw-event-tap CLASS"
                    .to_string(),
                "route external per-event delivery through the bounded \
                 `ExternalEventSinkPolicy` / native-sink seam in \
                 `substrate/external_event_sink/` instead of a new below-seam \
                 observer callback"
                    .to_string(),
            ));
        }
    }
    hits
}

/// Detect the CLASS: an `extern "C" fn(*mut c_void, *const c_char)` used as a
/// raw/signed-event *ingest observer/tap*. Requires BOTH the C-ABI per-event
/// callback signature AND an observer/tap intent token on the same line, so an
/// unrelated `extern "C"` callback is not a false positive.
fn class_violation_col(line: &str) -> Option<usize> {
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
    ];
    let lower = line.to_ascii_lowercase();
    if INTENT.iter().any(|t| lower.contains(t)) {
        return line.find("extern").or(Some(0));
    }
    None
}

/// True iff the rule should scan `path`.
///
/// Scope: workspace production Rust source under BOTH `crates/` and `apps/`
/// (the old A5 rule this replaces also covered `apps/`, where an app could
/// re-introduce a below-seam tap). Excludes:
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
/// lives. Used to exempt the sanctioned module from the CLASS check (the named
/// banned tokens still apply everywhere).
pub fn in_sink_module(path: &std::path::Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    s.contains("/external_event_sink/")
        || s.ends_with("/external_event_sink.rs")
        || s.ends_with("external_event_sink.rs")
}
