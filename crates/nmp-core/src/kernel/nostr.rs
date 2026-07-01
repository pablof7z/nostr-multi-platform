//! Pure Nostr-protocol helpers used by the kernel's event processing path.
//!
//! Contains event-parsing utilities (`parse_profile`, `parse_relay_list`),
//! display helpers (`short_hex`,
//! `avatar_color`, `truncate`, `initials`), and predicate helpers
//! (`is_hex_pubkey`, `event_references`). All functions are `pub(super)` or
//! `pub(crate)` — they are internal kernel implementation details, not public
//! NMP API.

use super::Deserialize;
// `DateTime`, `Local`, `SystemTime` are only consumed by `now_hms` below,
// `#[cfg(feature = "native")]` — the import is gated to match so
// `--no-default-features` (wasm32) compiles.
#[cfg(feature = "native")]
use super::{DateTime, Local, SystemTime};
use nmp_signer_iface::SignedEvent;

#[derive(Clone, Debug, Deserialize)]
pub(super) struct NostrEvent {
    pub(super) id: String,
    pub(super) pubkey: String,
    pub(super) created_at: u64,
    pub(super) kind: u32,
    pub(super) tags: Vec<Vec<String>>,
    pub(super) content: String,
    /// Schnorr signature (hex). Present in all valid NIP-01 events.
    /// Default to empty string so legacy test fixtures without `sig` still parse.
    #[serde(default)]
    pub(super) sig: String,
}

// ADR-0057 PR 2 — `parse_profile` + `ProfileContent` (the kind:0 metadata
// decoder) moved out of the kernel to `nmp_nip01::Kind0Parser` (the registered
// `IngestParser` that writes the capability-owned `nmp_nip01::ProfileCache`).
// The kernel no longer parses kind:0; it reads `crate::substrate::ProfileView`
// through `Kernel::profile_lookup()` (D0).

pub(super) fn signed_event_to_nostr(event: &SignedEvent) -> NostrEvent {
    NostrEvent {
        id: event.id.clone(),
        pubkey: event.unsigned.pubkey.clone(),
        created_at: event.unsigned.created_at,
        kind: event.unsigned.kind,
        tags: event.unsigned.tags.clone(),
        content: event.unsigned.content.clone(),
        sig: event.sig.clone(),
    }
}

pub(super) fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

pub fn is_hex_pubkey(value: &str) -> bool {
    value.len() == 64 && value.chars().all(|c| c.is_ascii_hexdigit())
}

pub fn is_hex_id(value: &str) -> bool {
    is_hex_pubkey(value)
}

// V-112 (ADR-0042): the NIP-10 thread-tag helpers (`event_references`,
// `referenced_event_ids`, `root_event_id`, `first_event_ref`,
// `marked_event_ref`) were deleted — their only consumers were the legacy
// `thread_items()` / `open_view_pins()` thread-hydration paths, retired with
// the author/thread view stack. Thread composition is app-side now
// (per-app FlatFeed over the generic `open_interest` seam).

pub(super) fn short_hex(value: &str) -> String {
    if value.len() < 12 {
        value.to_string()
    } else {
        format!("{}..{}", &value[..6], &value[value.len() - 6..])
    }
}

pub(super) fn truncate(value: &str, limit: usize) -> String {
    let mut out = String::new();
    for ch in value.chars().take(limit) {
        out.push(ch);
    }
    if value.chars().count() > limit {
        out.push_str("...");
    }
    out
}

// `chrono::Local` is the local-timezone reader; it lives behind chrono's
// `clock` feature, which `nmp-core` gates to `native` in Cargo.toml.
// Wall-clock display strings only appear on the FFI snapshot surface (whose
// callers are themselves native), so the helpers can also be `native`-only.
// V-01 Phase 1c: under `--no-default-features` the two call sites
// (`now_hms` in `status.rs`) are gated to match — the diagnostic strings
// drop out alongside the FFI module.
//
// `format_timestamp` deleted by ADR-0032 / V-115 F4: publish_outbox now
// emits raw `created_at` (Unix seconds); shells format timestamps locally.
#[cfg(feature = "native")]
pub(super) fn now_hms() -> String {
    let now = SystemTime::now(); // doctrine-allow: D9 — native-only diagnostic display helper; not reducer policy
    let datetime: DateTime<Local> = DateTime::<Local>::from(now);
    datetime.format("%H:%M:%S").to_string()
}
