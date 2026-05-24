//! Pure Nostr-protocol helpers used by the kernel's event processing path.
//!
//! Contains event-parsing utilities (`parse_profile`, `parse_relay_list`),
//! timeline diffing (`diff_items`), display helpers (`short_hex`,
//! `avatar_color`, `truncate`, `initials`), and predicate helpers
//! (`is_hex_pubkey`, `event_references`). All functions are `pub(super)` or
//! `pub(crate)` — they are internal kernel implementation details, not public
//! NMP API.

use super::{Deserialize, Profile, TimelineItem, HashMap, AuthorRelayList, HashSet, StoredEvent, BTreeSet};
// `UNIX_EPOCH`, `Duration`, `DateTime`, `Local`, `SystemTime` are only consumed
// by `format_timestamp` / `now_hms` below, both `#[cfg(feature = "native")]` —
// the imports are gated to match so `--no-default-features` (wasm32) compiles.
#[cfg(feature = "native")]
use super::{UNIX_EPOCH, Duration, DateTime, Local, SystemTime};
use crate::display::display_name_initials;
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
    /// `commands::zap_lnurl::lnurl_to_well_known_url`).
    pub(super) lud06: Option<String>,
}

pub(super) fn parse_profile(event: &NostrEvent) -> Profile {
    let parsed = serde_json::from_str::<ProfileContent>(&event.content).unwrap_or_default();
    let display = parsed
        .display_name
        .or(parsed.display_name_camel)
        .or(parsed.name)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| short_pubkey_display(&event.pubkey));
    Profile {
        event_id: event.id.clone(),
        created_at: event.created_at,
        avatar_initials: display_name_initials(&display),
        avatar_color: avatar_color(&event.pubkey),
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

pub(super) fn parse_profile_intent(event: &SignedEvent) -> Option<Profile> {
    if event.unsigned.kind != 0 {
        return None;
    }
    let event = signed_event_to_nostr(event);
    Some(parse_profile(&event))
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

pub(super) fn diff_items(
    previous: &[TimelineItem],
    current: &[TimelineItem],
) -> (Vec<TimelineItem>, Vec<TimelineItem>, Vec<String>) {
    let previous_by_id = previous
        .iter()
        .map(|item| (item.id.as_str(), item))
        .collect::<HashMap<_, _>>();
    let current_by_id = current
        .iter()
        .map(|item| (item.id.as_str(), item))
        .collect::<HashMap<_, _>>();

    let inserted = current
        .iter()
        .filter(|item| !previous_by_id.contains_key(item.id.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let updated = current
        .iter()
        .filter(|item| {
            previous_by_id
                .get(item.id.as_str())
                .is_some_and(|previous| *previous != *item)
        })
        .cloned()
        .collect::<Vec<_>>();
    let removed = previous
        .iter()
        .filter(|item| !current_by_id.contains_key(item.id.as_str()))
        .map(|item| item.id.clone())
        .collect::<Vec<_>>();

    (inserted, updated, removed)
}

pub(super) fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

pub(crate) fn is_hex_pubkey(value: &str) -> bool {
    value.len() == 64 && value.chars().all(|c| c.is_ascii_hexdigit())
}

pub(crate) fn is_hex_id(value: &str) -> bool {
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

pub(super) fn event_references(event: &StoredEvent, event_id: &str) -> bool {
    event.tags.iter().any(|tag| {
        tag.first().map(String::as_str) == Some("e") && tag.get(1).is_some_and(|id| id == event_id)
    })
}

pub(super) fn referenced_event_ids(event: &StoredEvent) -> BTreeSet<String> {
    event
        .tags
        .iter()
        .filter_map(|tag| {
            if tag.first().map(String::as_str) == Some("e") {
                tag.get(1).filter(|id| is_hex_id(id)).cloned()
            } else {
                None
            }
        })
        .collect()
}

pub(super) fn root_event_id(event: &StoredEvent) -> Option<String> {
    marked_event_ref(event, "root")
}

pub(super) fn first_event_ref(event: &StoredEvent) -> Option<String> {
    event.tags.iter().find_map(|tag| {
        if tag.first().map(String::as_str) == Some("e") {
            tag.get(1).filter(|id| is_hex_id(id)).cloned()
        } else {
            None
        }
    })
}

pub(super) fn marked_event_ref(event: &StoredEvent, marker: &str) -> Option<String> {
    event.tags.iter().find_map(|tag| {
        if tag.first().map(String::as_str) == Some("e")
            && tag.get(3).map(String::as_str) == Some(marker)
        {
            tag.get(1).filter(|id| is_hex_id(id)).cloned()
        } else {
            None
        }
    })
}

pub(super) fn short_hex(value: &str) -> String {
    if value.len() < 12 {
        value.to_string()
    } else {
        format!("{}..{}", &value[..6], &value[value.len() - 6..])
    }
}

pub(super) fn short_pubkey_display(value: &str) -> String {
    if value.len() < 16 {
        value.to_string()
    } else {
        format!("npub {}..{}", &value[..8], &value[value.len() - 8..])
    }
}

/// V-28 thin-shell: `<first8>…<last8>` abbreviation used by `TimelineItem`'s
/// `author_pubkey_short` and `short_id` fields. Deliberate micro-duplication of
/// `nmp_nip01::timeline_projection::pubkey_display` /
/// `nmp_nip17::display::pubkey_short` /
/// `nmp_nip29::projection::group_chat::pubkey_display` so all surfaces speak
/// the same dialect when abbreviating hex identifiers (the same author /
/// event id renders identically in NIP-01 timeline, NIP-17 DMs, NIP-29 group
/// rows, and Chirp's compose reply banner). NIP crates do not depend on each
/// other just to share trivial display helpers — see V-22 / V-25 / V-27
/// rationale. The load-bearing property is that the **algorithm stays
/// identical** across surfaces. Distinct from `short_pubkey_display` above
/// which carries the `npub ` prefix and `..` separator used by the kernel's
/// own author display fallback.
pub(super) fn short_hex_display(value: &str) -> String {
    if value.len() < 16 {
        value.to_string()
    } else {
        format!("{}…{}", &value[..8], &value[value.len() - 8..])
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

pub(super) fn avatar_color(pubkey: &str) -> String {
    // djb2 over the last 6 bytes of the hex string — byte-identical to the
    // canonical algorithm in nmp_nip17::display::avatar_color_hex, nmp_nip29,
    // and nmp_nip01. All surfaces must produce the same colour for the same
    // pubkey (same author in timeline vs DM vs group chat).
    let bytes = pubkey.as_bytes();
    let start = bytes.len().saturating_sub(6);
    let tail = &bytes[start..];
    let mut hash: u32 = 5381;
    for b in tail {
        hash = hash.wrapping_mul(33).wrapping_add(u32::from(*b));
    }
    format!("{:06X}", hash & 0x00FF_FFFF)
}

// `chrono::Local` is the local-timezone reader; it lives behind chrono's
// `clock` feature, which `nmp-core` gates to `native` in Cargo.toml.
// Wall-clock display strings only appear on the FFI snapshot surface (whose
// callers are themselves native), so the helpers can also be `native`-only.
// V-01 Phase 1c: under `--no-default-features` the two call sites
// (`format_timestamp` in `update.rs`, `now_hms` in `status.rs`) are gated
// to match — the diagnostic strings drop out alongside the FFI module.
#[cfg(feature = "native")]
pub(super) fn format_timestamp(created_at: u64) -> String {
    let Some(system_time) = UNIX_EPOCH.checked_add(Duration::from_secs(created_at)) else {
        return created_at.to_string();
    };
    let datetime: DateTime<Local> = DateTime::<Local>::from(system_time);
    datetime.format("%b %-d %H:%M").to_string()
}

#[cfg(feature = "native")]
pub(super) fn now_hms() -> String {
    let now = SystemTime::now();
    let datetime: DateTime<Local> = DateTime::<Local>::from(now);
    datetime.format("%H:%M:%S").to_string()
}
