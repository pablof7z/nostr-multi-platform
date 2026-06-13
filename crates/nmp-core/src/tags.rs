//! Shared, kind-agnostic Nostr tag helpers + the NIP-10 reference parser.
//!
//! This module is the `getNip10References` / e-p-a-q tag-builder equivalent
//! from applesauce, refactored into NMP idiom. It lives in `nmp-core`
//! alongside [`crate::nip19`] and [`crate::nip21`] for the same reason those
//! do: it is a **protocol codec**, not a per-kind decoder or a domain noun.
//! D0 (`docs/design/kind-wrappers.md`) forbids the kernel knowing
//! "kind 30023 == article"; nothing here encodes any kind semantics — every
//! function is a pure transform over `&[Vec<String>]`. Per-kind decoders
//! (kind 7 → `ReactionRecord`, etc.) stay in their protocol crates.
//!
//! Both the per-NIP relation crates and the `nmp-relations` facade consume
//! these helpers so tag construction and NIP-10 interpretation are defined
//! exactly once.

use serde::{Deserialize, Serialize};

// ─── Tag constructors ────────────────────────────────────────────────────────

/// Build an `e` tag: `["e", <id>]`, optionally with a relay hint and a
/// NIP-10 marker (`"root"` / `"reply"` / `"mention"`).
///
/// NIP-10 marked form requires the relay slot to be present (possibly empty)
/// when a marker follows, so a `Some(marker)` always emits the 4-column form
/// `["e", id, relay_or_empty, marker]`.
#[must_use]
pub fn e_tag(id: &str, relay: Option<&str>, marker: Option<&str>) -> Vec<String> {
    match (relay, marker) {
        (_, Some(marker)) => vec![
            "e".to_string(),
            id.to_string(),
            relay.unwrap_or("").to_string(),
            marker.to_string(),
        ],
        (Some(relay), None) => vec!["e".to_string(), id.to_string(), relay.to_string()],
        (None, None) => vec!["e".to_string(), id.to_string()],
    }
}

/// Build a `p` tag: `["p", <pubkey>]`, optionally with a relay hint.
#[must_use]
pub fn p_tag(pubkey: &str, relay: Option<&str>) -> Vec<String> {
    match relay {
        Some(relay) => vec!["p".to_string(), pubkey.to_string(), relay.to_string()],
        None => vec!["p".to_string(), pubkey.to_string()],
    }
}

/// Build a NIP-33 `a` tag: `["a", "<kind>:<pubkey>:<d_tag>"]`, optionally with
/// a relay hint.
#[must_use]
pub fn a_tag(kind: u32, pubkey: &str, d_tag: &str, relay: Option<&str>) -> Vec<String> {
    let coord = format!("{kind}:{pubkey}:{d_tag}");
    match relay {
        Some(relay) => vec!["a".to_string(), coord, relay.to_string()],
        None => vec!["a".to_string(), coord],
    }
}

/// Build a NIP-18 `q` (quote) tag: `["q", <id>]`, optionally with a relay hint.
#[must_use]
pub fn q_tag(id: &str, relay: Option<&str>) -> Vec<String> {
    match relay {
        Some(relay) => vec!["q".to_string(), id.to_string(), relay.to_string()],
        None => vec!["q".to_string(), id.to_string()],
    }
}

// ─── Tag readers ─────────────────────────────────────────────────────────────

/// Return the second column of the first tag whose first column equals `key`.
///
/// Promoted here from the copy that was private to `nmp-nip23::decode` so
/// every protocol crate shares one implementation.
#[must_use]
pub fn first_tag_value<'a>(tags: &'a [Vec<String>], key: &str) -> Option<&'a str> {
    tags.iter()
        .find(|t| t.first().map(String::as_str) == Some(key))
        .and_then(|t| t.get(1))
        .map(String::as_str)
}

/// Return the second column of every tag whose first column equals `key`,
/// in document order.
#[must_use]
pub fn all_tag_values<'a>(tags: &'a [Vec<String>], key: &str) -> Vec<&'a str> {
    tags.iter()
        .filter(|t| t.first().map(String::as_str) == Some(key))
        .filter_map(|t| t.get(1))
        .map(String::as_str)
        .collect()
}

/// Per-account cap on the number of follows derived from a kind:3 contact
/// list. The kernel REQs at most this many follow-feed authors, so anything
/// downstream that rebuilds the active account's follow set (the router's
/// `timeline_authors`, the NIP-02 predicate producer, the `nmp.follow_list`
/// snapshot) MUST apply the same bound or it diverges from what the wire
/// actually subscribes to.
///
/// Substrate-generic: a cap is a number, not an app noun (D0). It lives here
/// in the kind-agnostic tag module so both `nmp-core` ingest and the
/// `nmp-nip02` observers reach the single source of truth without
/// `nmp-core → nmp-nip02` inversion. [`crate::relay`] re-exports this as its
/// historical `TIMELINE_AUTHOR_LIMIT` name; there is exactly one `500` in the
/// codebase.
pub const TIMELINE_AUTHOR_LIMIT: usize = 500;

/// Derive the active account's capped follow set from a kind:3 contact list's
/// tags — the **single source of truth** for "which follows count".
///
/// Semantics (must match `Kernel::ingest_contacts` exactly):
/// 1. Keep `["p", <value>, …]` tags whose value is a valid 64-hex pubkey
///    ([`crate::kernel::is_hex_pubkey`]) — malformed `p` entries are skipped.
/// 2. Preserve **document order**; do **not** dedup and do **not** sort
///    (the kernel collects into a `Vec`, so a duplicate `p` tag occupies a
///    cap slot exactly as it does on the wire).
/// 3. Take the first [`TIMELINE_AUTHOR_LIMIT`] survivors.
///
/// Returns owned `String`s so callers (kernel ingest and the two `nmp-nip02`
/// `KernelEventObserver`s) all consume an identical capped set without
/// re-implementing — and therefore re-diverging — the recipe.
#[must_use]
pub fn capped_contact_follows(tags: &[Vec<String>]) -> Vec<String> {
    tags.iter()
        .filter_map(|tag| {
            if tag.first().map(String::as_str) == Some("p") {
                tag.get(1)
                    .filter(|value| crate::kernel::is_hex_pubkey(value))
                    .cloned()
            } else {
                None
            }
        })
        .take(TIMELINE_AUTHOR_LIMIT)
        .collect()
}

// ─── NIP-02 kind:3 contact-list edit builders ────────────────────────────────

/// Return the FULL kind:3 tag set that results from adding a follow on `target`
/// to `current` — splicing ONLY the `p` section while preserving everything
/// else verbatim (issue #1246).
///
/// `current` is the active account's existing kind:3 tag set
/// (`Vec<Vec<String>>`), obtained from a confirmed-loaded kind:3 via
/// [`crate::kernel_reducer::KernelReducer::try_current_kind3_event`] (or the
/// native `Kernel::try_current_kind3_event`). Callers MUST confirm the kind:3
/// is loaded first — editing a not-yet-loaded list and re-publishing would
/// silently wipe the user's contacts.
///
/// Preservation contract:
/// - Every **non-`p`** tag (legacy relay-list `["r", …]`, `["d", …]`, etc.) is
///   carried through verbatim, in document order.
/// - Every existing `["p", pk, relay?, petname?]` entry keeps its relay-hint
///   (column 2) and petname (column 3) columns — the edit never strips them.
/// - Document order of all retained tags is preserved.
///
/// Idempotent: if a `p` tag for `target` (matched on column 1, the pubkey) is
/// already present, the set is returned unchanged — no duplicate, and the
/// existing entry's relay-hint / petname survive. Otherwise a bare
/// `["p", target]` is appended after the existing tags.
#[must_use]
pub fn kind3_tags_after_add(current: &[Vec<String>], target: &str) -> Vec<Vec<String>> {
    let mut tags: Vec<Vec<String>> = current.to_vec();
    let already_present = tags
        .iter()
        .any(|t| t.first().map(String::as_str) == Some("p") && t.get(1).map(String::as_str) == Some(target));
    if !already_present {
        tags.push(vec!["p".to_string(), target.to_string()]);
    }
    tags
}

/// Return the FULL kind:3 tag set that results from removing the follow on
/// `target` from `current` — dropping ONLY the matching `p` entries while
/// preserving everything else verbatim (issue #1246).
///
/// Drops every `["p", target, …]` entry of ANY arity (bare, relay-hinted, or
/// relay-hinted-with-petname) matched on column 1 (the pubkey). Every non-`p`
/// tag and every `p` tag for a different pubkey — including its relay-hint and
/// petname columns — is carried through verbatim in document order.
///
/// Idempotent: if no `p` tag for `target` is present, the set is returned
/// unchanged. Same must-be-loaded safety constraint as
/// [`kind3_tags_after_add`].
#[must_use]
pub fn kind3_tags_after_remove(current: &[Vec<String>], target: &str) -> Vec<Vec<String>> {
    current
        .iter()
        .filter(|t| {
            !(t.first().map(String::as_str) == Some("p")
                && t.get(1).map(String::as_str) == Some(target))
        })
        .cloned()
        .collect()
}

// ─── NIP-10 reply builder ────────────────────────────────────────────────────

/// Build the NIP-10 marked-form reply tag set for a new note that replies to
/// the event described by `parent_id` / `parent_author` / `parent_refs`.
///
/// This is the shared canonical implementation; both the `nmp-nip01` kind:1
/// builder and `KernelReducer::build_reply_tags` (the wasm write-path seam)
/// delegate to it so there is exactly one copy of the root-inheritance rule,
/// the p-tag dedup pass, and the relay-hint placement.
///
/// # Tag layout (NIP-10 marked form)
///
/// 1. `["e", root_id, relay_or_empty, "root"]` — thread root.
///    When `parent_refs.root` is `None` the parent IS the root, so this e-tag
///    carries `parent_id`. When `parent_refs.root` is `Some(root_ref)` the
///    root id and its stored relay hint are inherited from that ref.
/// 2. `["e", parent_id, relay_or_empty, "reply"]` — direct parent.
///    The relay column uses `relay_hint`.
/// 3. One `["p", pubkey]` per thread-participant: `parent_author` first, then
///    `parent_refs.mentioned_pubkeys`, de-duplicated in stable order.
///
/// `relay_hint` is applied to the reply e-tag and all p-tags; the root e-tag
/// uses the relay stored in `parent_refs.root` (or `relay_hint` when the
/// parent is the root and there is no prior relay annotation).
#[must_use]
pub fn reply_tags(
    parent_id: &str,
    parent_author: &str,
    parent_refs: &Nip10Refs,
    relay_hint: Option<&str>,
) -> Vec<Vec<String>> {
    let (root_id, root_relay): (&str, Option<&str>) = match parent_refs.root.as_ref() {
        Some(root) => (root.id.as_str(), root.relay.as_deref()),
        None => (parent_id, relay_hint),
    };

    // Build the p-tag pubkey set: parent author first, then anyone the parent
    // was already notifying, de-duplicated, stable order.
    let mut pubkeys: Vec<&str> = Vec::with_capacity(1 + parent_refs.mentioned_pubkeys.len());
    pubkeys.push(parent_author);
    for pk in &parent_refs.mentioned_pubkeys {
        if !pubkeys.iter().any(|p| *p == pk.as_str()) {
            pubkeys.push(pk.as_str());
        }
    }

    let mut tags = Vec::with_capacity(2 + pubkeys.len());
    tags.push(e_tag(root_id, root_relay, Some("root")));
    tags.push(e_tag(parent_id, relay_hint, Some("reply")));
    for pk in pubkeys {
        tags.push(p_tag(pk, relay_hint));
    }
    tags
}

// ─── NIP-25 reaction builder ─────────────────────────────────────────────────

/// Build NIP-25 kind:7 reaction tags and normalised content for
/// `target_event_id`.
///
/// Returns `None` when `target_event_id` is not a valid 64-char hex event id
/// (same gate as `crate::kernel::is_hex_id`). Otherwise returns
/// `Some((tags, content))` where:
/// - `tags` = `[["e", target_event_id], ["p", author]?]`
/// - `content` = `reaction` normalised to `"+"` when blank
///
/// `author` is `None` when the target event's author is absent from the
/// caller's read-cache; the e-tag-only reaction is still valid NIP-25 (D6:
/// degrade, never refuse the publish).
///
/// Shared canonical implementation; both `KernelReducer::build_reaction_draft`
/// (wasm write-path) and native `actor::commands::publish::react` delegate
/// here so tag logic is defined once and cannot silently drift.
#[must_use]
pub fn reaction_tags(
    target_event_id: &str,
    author: Option<&str>,
    reaction: &str,
) -> Option<(Vec<Vec<String>>, String)> {
    if !crate::kernel::is_hex_id(target_event_id) {
        return None;
    }
    let content = if reaction.trim().is_empty() { "+".to_string() } else { reaction.to_string() };
    let mut tags = vec![vec!["e".to_string(), target_event_id.to_string()]];
    if let Some(pk) = author {
        tags.push(vec!["p".to_string(), pk.to_string()]);
    }
    Some((tags, content))
}

// ─── NIP-10 reference parser ─────────────────────────────────────────────────

/// A single `e`-tag reference: the pointed-to event id, plus the optional
/// relay hint and NIP-10 marker that accompanied it.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct EventRef {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub marker: Option<String>,
}

/// The NIP-10 thread references decoded from an event's tags — the NMP
/// equivalent of applesauce's `getNip10References`.
///
/// `root` is the thread root, `reply` is the direct parent this event is
/// replying to (the `replyingTo$` target), `mentions` are quoted/mentioned
/// events, and `mentioned_pubkeys` carries the `p` tags so a reply builder
/// can re-notify the thread participants per NIP-10.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct Nip10Refs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<EventRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reply: Option<EventRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mentions: Vec<EventRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mentioned_pubkeys: Vec<String>,
}

impl Nip10Refs {
    /// True when the event carries no root and no reply marker — i.e. it is a
    /// thread root itself, not a reply (mirrors applesauce `Note.isRoot`).
    #[must_use]
    pub fn is_root(&self) -> bool {
        self.root.is_none() && self.reply.is_none()
    }

    /// True when the event replies to something (mirrors `Note.isReply`).
    #[must_use]
    pub fn is_reply(&self) -> bool {
        self.reply.is_some()
    }
}

fn e_ref_from_tag(tag: &[String]) -> Option<EventRef> {
    let id = tag.get(1)?.clone();
    if id.is_empty() {
        return None;
    }
    let relay = tag.get(2).filter(|s| !s.is_empty()).cloned();
    let marker = tag.get(3).filter(|s| !s.is_empty()).cloned();
    Some(EventRef { id, relay, marker })
}

/// Parse NIP-10 thread references from raw tags.
///
/// Supports the preferred **marked** form (`["e", id, relay, "root|reply|
/// mention"]`) and falls back to the deprecated **positional** convention
/// when no markers are present:
/// - 0 `e` tags → not a reply.
/// - 1 `e` tag → that event is both the root and the direct parent.
/// - ≥2 `e` tags → first is root, last is the direct parent, middle are
///   mentions.
///
/// When a `root` marker is present but no `reply` marker is, the reply target
/// is the root (a top-level reply to the thread root) — matching the common
/// client interpretation and applesauce's behaviour.
#[must_use]
pub fn parse_nip10(tags: &[Vec<String>]) -> Nip10Refs {
    let e_tags: Vec<&Vec<String>> = tags
        .iter()
        .filter(|t| t.first().map(String::as_str) == Some("e"))
        .collect();

    let mentioned_pubkeys: Vec<String> = all_tag_values(tags, "p")
        .into_iter()
        .map(str::to_string)
        .collect();

    let has_marker = e_tags.iter().any(|t| {
        matches!(
            t.get(3).map(String::as_str),
            Some("root" | "reply" | "mention")
        )
    });

    if has_marker {
        let mut refs = Nip10Refs {
            mentioned_pubkeys,
            ..Default::default()
        };
        for tag in &e_tags {
            let Some(eref) = e_ref_from_tag(tag) else {
                continue;
            };
            match eref.marker.as_deref() {
                Some("root") => {
                    if refs.root.is_none() {
                        refs.root = Some(eref);
                    }
                }
                Some("reply") => {
                    if refs.reply.is_none() {
                        refs.reply = Some(eref);
                    }
                }
                _ => refs.mentions.push(eref),
            }
        }
        // Top-level reply to a root: a "root" with no explicit "reply".
        if refs.reply.is_none() {
            refs.reply = refs.root.clone();
        }
        return refs;
    }

    // Positional fallback (deprecated NIP-10 form).
    let resolved: Vec<EventRef> = e_tags.iter().filter_map(|t| e_ref_from_tag(t)).collect();

    match resolved.len() {
        0 => Nip10Refs {
            mentioned_pubkeys,
            ..Default::default()
        },
        1 => Nip10Refs {
            root: Some(resolved[0].clone()),
            reply: Some(resolved[0].clone()),
            mentions: Vec::new(),
            mentioned_pubkeys,
        },
        n => Nip10Refs {
            root: Some(resolved[0].clone()),
            reply: Some(resolved[n - 1].clone()),
            mentions: resolved[1..n - 1].to_vec(),
            mentioned_pubkeys,
        },
    }
}

// Unit tests live in sibling files to keep this module under the 500-line
// ceiling. `use super::*` in each provides the same namespace access an inline
// `mod tests` would. `tags_tests.rs` covers the constructors, readers, NIP-10
// parser, and the `capped_contact_follows` follow-cap; `tags_reply_tests.rs`
// covers `reply_tags`.
#[cfg(test)]
#[path = "tags_tests.rs"]
mod tags_tests;

#[cfg(test)]
#[path = "tags_reply_tests.rs"]
mod reply_tests;
