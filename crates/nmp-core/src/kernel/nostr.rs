//! Pure Nostr-protocol helpers used by the kernel's event processing path.
//!
//! Contains event-parsing utilities (`parse_profile`, `parse_relay_list`),
//! display helpers (`short_hex`,
//! `avatar_color`, `truncate`, `initials`), and predicate helpers
//! (`is_hex_pubkey`, `event_references`). All functions are `pub(super)` or
//! `pub(crate)` — they are internal kernel implementation details, not public
//! NMP API.

use super::types::AuthorRelayList;
use super::{Deserialize, HashSet, Profile};
// `DateTime`, `Local`, `SystemTime` are only consumed by `now_hms` below,
// `#[cfg(feature = "native")]` — the import is gated to match so
// `--no-default-features` (wasm32) compiles.
#[cfg(feature = "native")]
use super::{DateTime, Local, SystemTime};
use crate::substrate::SignedEvent;

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

#[derive(Default, Deserialize)]
pub(super) struct ProfileContent {
    pub(super) name: Option<String>,
    pub(super) display_name: Option<String>,
    #[serde(rename = "displayName")]
    pub(super) display_name_camel: Option<String>,
    pub(super) picture: Option<String>,
    pub(super) nip05: Option<String>,
    pub(super) about: Option<String>,
    /// NIP-57 lightning address (`user@domain`). Preferred over `lud06` when
    /// both are present (most modern wallets emit `lud16`). Surfaced into
    /// `Profile::lnurl` so the zap UI can pre-populate `ZapInput.lnurl`
    /// without Swift parsing raw kind:0 metadata (thin-shell rule).
    pub(super) lud16: Option<String>,
    /// NIP-57 LNURL-pay bech32 (`lnurl1…`). Legacy/alternate to `lud16`;
    /// surfaced when `lud16` is absent. Both feed the same `Profile::lnurl`
    /// optional field — the zap handler accepts either shape (see
    /// `nmp_nip57::lnurl::lnurl_to_well_known_url`).
    pub(super) lud06: Option<String>,
}

pub(super) fn parse_profile(event: &NostrEvent) -> Profile {
    let parsed = serde_json::from_str::<ProfileContent>(&event.content).unwrap_or_default();
    // Verbatim display-name value from kind:0; empty string when the
    // parsed metadata carries none of `display_name` / `displayName` /
    // `name` (aim.md §2 — no `short_npub` fallback is substituted; the
    // projection boundary converts `""` into `Option::None`).
    let display = parsed
        .display_name
        .or(parsed.display_name_camel)
        .or(parsed.name)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_default();
    Profile {
        event_id: event.id.clone(),
        created_at: event.created_at,
        display,
        picture_url: parsed.picture.filter(|value| value.starts_with("http")),
        nip05: parsed.nip05.unwrap_or_default(),
        about: parsed.about.unwrap_or_default(),
        // NIP-57 — prefer `lud16` (lightning address) over `lud06` (LNURL
        // bech32). Both empty strings filter out so the zap button stays
        // disabled when a kind:0 carries the key with an empty value.
        lnurl: parsed
            .lud16
            .filter(|s| !s.trim().is_empty())
            .or_else(|| parsed.lud06.filter(|s| !s.trim().is_empty())),
    }
}

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

pub(super) fn parse_relay_list(
    event_id: &str,
    created_at: u64,
    tags: &[Vec<String>],
) -> AuthorRelayList {
    let mut list = AuthorRelayList {
        event_id: event_id.to_string(),
        created_at,
        ..AuthorRelayList::default()
    };
    let mut seen = HashSet::new();

    for tag in tags {
        if tag.first().map(String::as_str) != Some("r") {
            continue;
        }
        let Some(url) = tag.get(1).filter(|url| url.starts_with("wss://")) else {
            continue;
        };
        let marker = tag.get(2).map_or("both", String::as_str);
        let key = format!("{url}:{marker}");
        if !seen.insert(key) {
            continue;
        }
        match marker {
            "read" => list.read_relays.push(url.clone()),
            "write" => list.write_relays.push(url.clone()),
            _ => list.both_relays.push(url.clone()),
        }
    }

    list
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
    let now = SystemTime::now();
    let datetime: DateTime<Local> = DateTime::<Local>::from(now);
    datetime.format("%H:%M:%S").to_string()
}
