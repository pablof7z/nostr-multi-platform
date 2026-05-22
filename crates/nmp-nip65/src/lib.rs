//! `nmp-nip65` — NIP-65 relay-list (kind:10002) publish path.
//!
//! # Why this exists
//!
//! The kernel already INGESTS kind:10002 events (see
//! `nmp-core::kernel::ingest::relay_list`) to populate the
//! `author_relay_lists` cache that drives the NIP-65 outbox resolver. That
//! cache is what every publish + REQ fan-out consults for "where does this
//! author read/write?".
//!
//! But the actor's local `AddRelay` / `RemoveRelay` arms only mutate the
//! `RelayEditRow` projection and dial / drop sockets — they never publish a
//! new kind:10002 that reflects the change. The result is asymmetric:
//!
//! * a user removes a defunct relay → no kind:10002 update → other clients
//!   still fan REQs and publishes out to a dead host;
//! * a user adds a new relay → never advertised → contacts have no signal
//!   to read or write there.
//!
//! `nmp.nip65.publish_relay_list` closes that loop: a host (or the actor's
//! own AddRelay/RemoveRelay arms, via a sibling in-tree helper) publishes
//! a kind:10002 reflecting the user's intended relay set. The kernel then
//! ingests its own publish exactly as any other client's, keeping the
//! `author_relay_lists` cache for the active account in sync with the
//! `RelayEditRow` projection without a special case.
//!
//! # Tag shape — NIP-65
//!
//! kind:10002 carries `["r", <wss-url>]` tags. The optional third element
//! is the role marker:
//!
//! * `["r", <url>]`           → read + write (default, parsed as "both")
//! * `["r", <url>, "read"]`   → read-only
//! * `["r", <url>, "write"]`  → write-only
//!
//! Any third-element value other than `"read"` / `"write"` is parsed by the
//! kernel as "both" (see `nmp-core::kernel::nostr::parse_relay_list` at line
//! 158: `let marker = tag.get(2).map(String::as_str).unwrap_or("both")`).
//! The builder here MUST agree with that parser so a publish → ingest round
//! trip is lossless.
//!
//! # Routing
//!
//! kind:10002 is itself a NIP-65 replaceable event (`10000 ≤ kind < 20000`).
//! The executor enqueues [`ActorCommand::PublishUnsignedEvent`] — the
//! kernel's Auto path — so the very first kind:10002 for a freshly-created
//! account hits the bootstrap discovery relays (no chicken-and-egg), and
//! later updates land on the author's own write set.
//!
//! # D7 — `created_at` sentinel
//!
//! The unsigned event is built with `created_at: 0`. The actor re-stamps
//! it from `kernel.now_secs()` before signing (see the
//! `PublishUnsignedEvent` arm in `nmp-core::actor::dispatch`); this crate
//! never reads the system clock.
//!
//! # D0 — namespace
//!
//! The action namespace is `nmp.nip65.publish_relay_list` — mirroring
//! `nmp.nip17.publish_relay_list` (kind:10050). The kernel sees only the
//! namespace string; it carries no NIP-65 nouns.

use nmp_core::substrate::{ActionContext, ActionModule, ActionRejection, UnsignedEvent};
use nmp_core::{canonical_relay_url, ActorCommand};
use serde::{Deserialize, Serialize};

/// NIP-65 kind: the relay list — read/write outbox/inbox advertisement.
const KIND_RELAY_LIST: u32 = 10002;

/// Per-relay role marker for a NIP-65 entry.
///
/// The wire format on kind:10002 is:
/// * [`Both`] → tag `["r", url]` with no third element (the default).
/// * [`Read`] → tag `["r", url, "read"]`.
/// * [`Write`] → tag `["r", url, "write"]`.
///
/// The kernel parser treats *any* third-element string other than `"read"`
/// or `"write"` as "both", but to keep the publish → ingest round-trip
/// stable in the canonical case the builder OMITS the third element for
/// [`Both`] rather than emitting `"both"`.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RelayMarker {
    /// Read + write. Wire form: `["r", url]` (no marker).
    #[default]
    Both,
    /// Read-only. Wire form: `["r", url, "read"]`.
    Read,
    /// Write-only. Wire form: `["r", url, "write"]`.
    Write,
}

/// One relay entry in the user's NIP-65 outbox/inbox advertisement.
///
/// `url` is canonicalised by the builder (lowercase scheme+host, trailing
/// `/` stripped on empty path). Non-`wss://` / `ws://` URLs are dropped —
/// the kernel's ingest parser requires `wss://`, and the builder mirrors
/// that gate so a publish → ingest round-trip is stable.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct RelayListEntry {
    /// Relay URL. Canonicalised before being written to the tag.
    pub url: String,
    /// Read/write role marker. Defaults to [`RelayMarker::Both`].
    #[serde(default)]
    pub marker: RelayMarker,
}

/// Build a NIP-65 kind:10002 relay-list **unsigned** event from an explicit
/// list of [`RelayListEntry`] values.
///
/// Per NIP-65, each entry becomes an `["r", <url>]` tag with an optional
/// `"read"` / `"write"` third element. The default marker [`RelayMarker::Both`]
/// omits the third element entirely (matching the kernel parser's
/// `.unwrap_or("both")` branch); the explicit `"read"` / `"write"` markers
/// emit the marker verbatim.
///
/// URLs are canonicalised via [`nmp_core::canonical_relay_url`] (lowercase
/// scheme+host, trailing-`/` stripped on empty path) and deduplicated by
/// canonical URL in first-seen order. URLs that do not parse as `ws://` or
/// `wss://` are dropped — this matches the ingest parser's `wss://` gate so a
/// build → ingest round-trip is stable. (`ws://` is accepted by the
/// canonicaliser but will be SKIPPED by the kernel parser, which requires
/// `wss://`; callers should configure `wss://`.)
///
/// Dedup is by canonical URL only — two entries for the same host with
/// different markers collapse to the *first* marker seen. Callers that
/// need to express "both directions" should set [`RelayMarker::Both`]
/// once; emitting two tags (one `read`, one `write`) for the same host is
/// not what NIP-65 specifies and the kernel parser would not re-merge
/// them correctly.
///
/// The returned event:
/// * has `kind = 10002`,
/// * has `created_at = 0` — the D7 sentinel; the actor re-stamps it,
/// * has an empty `pubkey` — the actor derives it from the signing keys at
///   sign time (this mirrors `nmp_nip17::build_dm_relay_list_event` and the
///   NIP-29 builders; the build half is pubkey-agnostic).
pub fn build_relay_list_event(entries: &[RelayListEntry]) -> UnsignedEvent {
    let mut tags: Vec<Vec<String>> = Vec::with_capacity(entries.len());
    let mut seen = std::collections::HashSet::new();
    for entry in entries {
        let Some(canonical) = canonical_relay_url(&entry.url) else {
            continue;
        };
        if !seen.insert(canonical.clone()) {
            continue;
        }
        let tag = match entry.marker {
            RelayMarker::Both => vec!["r".to_string(), canonical],
            RelayMarker::Read => vec!["r".to_string(), canonical, "read".to_string()],
            RelayMarker::Write => vec!["r".to_string(), canonical, "write".to_string()],
        };
        tags.push(tag);
    }
    UnsignedEvent {
        // Empty placeholder — the actor re-derives the pubkey from the
        // signing key at sign time (see `ActorCommand::PublishUnsignedEvent`).
        pubkey: String::new(),
        kind: KIND_RELAY_LIST,
        tags,
        content: String::new(),
        // D7 sentinel — the actor re-stamps from `kernel.now_secs()`.
        created_at: 0,
    }
}

/// Wire shape for `nmp.nip65.publish_relay_list` — the JSON a host passes to
/// `nmp_app_dispatch_action`.
///
/// `relays` is the user's full NIP-65 relay set. The host is the source of
/// truth here: it reads the user's configured relays from its own UI state
/// (typically the same `RelayEditRow` projection the kernel exposes) and
/// hands them in. Keeping the action stateless (no kernel reads in the
/// executor) is consistent with the rest of the action surface — the
/// executor closure receives only the JSON, the correlation id, and a send
/// callback.
///
/// The auto-trigger path from `actor::dispatch::AddRelay` / `RemoveRelay`
/// is sibling to this action, NOT a caller of it: the actor reads its own
/// `RelayEditRow` projection and calls `build_relay_list_event` directly,
/// because `ActionContext` does not carry kernel state and `execute`'s
/// signature is `(action, correlation_id, send)`. Both paths converge on
/// the same on-wire kind:10002 shape via the shared builder above.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PublishRelayListInput {
    /// Relays to advertise as the user's NIP-65 relay set. Canonicalised
    /// and deduped by the builder; URLs that do not parse as `wss://` /
    /// `ws://` are dropped.
    pub relays: Vec<RelayListEntry>,
}

/// The `nmp.nip65.publish_relay_list` [`ActionModule`] — a pure shape validator.
///
/// Mirrors `nmp_nip17::PublishDmRelayListAction`'s discipline: `start` is
/// a side-effect-free shape check; the actual sign + publish happens on
/// the actor thread (D7) via `ActorCommand::PublishUnsignedEvent`.
pub struct PublishRelayListAction;

impl ActionModule for PublishRelayListAction {
    const NAMESPACE: &'static str = "nmp.nip65.publish_relay_list";
    type Action = PublishRelayListInput;

    /// Reject an empty relay set — a kind:10002 with zero `r` tags is the
    /// canonical "I cleared my NIP-65 metadata" signal in
    /// `ingest_relay_list` (`nmp-core::kernel::ingest::relay_list`), which
    /// REMOVES the cache entry and forces every subsequent fan-out for
    /// this author through the cold-start bootstrap discovery seed. That
    /// is a destructive operation and should not be reachable via the
    /// "publish my list" verb. A host wanting to explicitly clear the
    /// list needs its own explicit verb (this v1 does not ship one).
    fn start(_ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        if action.relays.is_empty() {
            return Err(ActionRejection::Invalid(
                "empty NIP-65 relay list — refusing to publish a kind:10002 \
                 that would clear the author_relay_lists cache for this user"
                    .into(),
            ));
        }
        // Reject input that produces zero canonical tags (every URL was
        // malformed). Reaching the actor with a zero-tag event would emit
        // a valid kind:10002 that clears the cache — the same destructive
        // op the empty-input guard above blocks.
        let event = build_relay_list_event(&action.relays);
        if event.tags.is_empty() {
            return Err(ActionRejection::Invalid(
                "no canonical wss:// / ws:// relay URLs in input".into(),
            ));
        }
        Ok(())
    }

    fn execute(
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        let event = build_relay_list_event(&action.relays);
        // kind:10002 is a NIP-65 replaceable event — route through the
        // kernel's Auto path. For the *first* kind:10002 the author ever
        // publishes there is no NIP-65 outbox yet, so Auto falls back to
        // the bootstrap discovery relays (chicken-and-egg solved). For
        // updates, the existing outbox is used.
        //
        // Thread the registry-minted `correlation_id` so the publish
        // engine reports it in `action_results` and the host spinner that
        // fired on `dispatch_action` can be cleared with a terminal
        // verdict. Without this the dispatch arm never records a
        // terminal stage and the spinner hangs forever.
        send(ActorCommand::PublishUnsignedEvent {
            event,
            correlation_id: Some(correlation_id.to_string()),
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(url: &str, marker: RelayMarker) -> RelayListEntry {
        RelayListEntry {
            url: url.to_string(),
            marker,
        }
    }

    // --- builder ---------------------------------------------------------

    #[test]
    fn build_produces_kind_10002() {
        let event = build_relay_list_event(&[entry("wss://relay.example", RelayMarker::Both)]);
        assert_eq!(event.kind, 10002);
    }

    #[test]
    fn build_uses_created_at_zero_sentinel() {
        let event = build_relay_list_event(&[entry("wss://relay.example", RelayMarker::Both)]);
        assert_eq!(
            event.created_at, 0,
            "D7: created_at is the 0 sentinel — the actor re-stamps it"
        );
    }

    #[test]
    fn build_leaves_pubkey_empty_for_actor_to_fill() {
        let event = build_relay_list_event(&[entry("wss://relay.example", RelayMarker::Both)]);
        assert!(
            event.pubkey.is_empty(),
            "pubkey is a placeholder — the actor derives it from the signing key"
        );
    }

    #[test]
    fn build_both_marker_omits_third_tag_element() {
        // NIP-65: `["r", url]` (no third element) is the canonical
        // "read + write" form. The kernel parser's `.unwrap_or("both")`
        // branch hits this directly.
        let event = build_relay_list_event(&[entry("wss://relay.example", RelayMarker::Both)]);
        assert_eq!(
            event.tags,
            vec![vec!["r".to_string(), "wss://relay.example".to_string()]],
        );
    }

    #[test]
    fn build_read_marker_emits_read_third_element() {
        let event = build_relay_list_event(&[entry("wss://relay.example", RelayMarker::Read)]);
        assert_eq!(
            event.tags,
            vec![vec![
                "r".to_string(),
                "wss://relay.example".to_string(),
                "read".to_string()
            ]],
        );
    }

    #[test]
    fn build_write_marker_emits_write_third_element() {
        let event = build_relay_list_event(&[entry("wss://relay.example", RelayMarker::Write)]);
        assert_eq!(
            event.tags,
            vec![vec![
                "r".to_string(),
                "wss://relay.example".to_string(),
                "write".to_string()
            ]],
        );
    }

    #[test]
    fn build_uses_r_marker_not_relay_marker() {
        // NIP-65 § uses `["r", url]` tags. Using `["relay", ...]` would be
        // a kind:10050 NIP-17 shape; the kernel's `parse_relay_list` would
        // skip every tag and the round-trip would silently produce an
        // empty cache entry — exactly the kind of leak this test pins.
        let event = build_relay_list_event(&[entry("wss://relay.example", RelayMarker::Both)]);
        for tag in &event.tags {
            assert_eq!(
                tag.first().map(String::as_str),
                Some("r"),
                "NIP-65 tag marker is 'r' (not 'relay' — that is NIP-17 / kind:10050)"
            );
        }
    }

    #[test]
    fn build_preserves_input_order() {
        let event = build_relay_list_event(&[
            entry("wss://b.example", RelayMarker::Both),
            entry("wss://a.example", RelayMarker::Both),
            entry("wss://c.example", RelayMarker::Both),
        ]);
        let urls: Vec<&String> = event.tags.iter().map(|t| &t[1]).collect();
        assert_eq!(
            urls,
            vec!["wss://b.example", "wss://a.example", "wss://c.example"]
        );
    }

    #[test]
    fn build_dedups_equivalent_urls() {
        // `wss://Relay.Example/` and `wss://relay.example` canonicalise to
        // the same value — only one tag should appear. Dedup is by
        // canonical URL only, so the FIRST marker wins.
        let event = build_relay_list_event(&[
            entry("wss://Relay.Example/", RelayMarker::Read),
            entry("wss://relay.example", RelayMarker::Write),
        ]);
        assert_eq!(event.tags.len(), 1);
        assert_eq!(
            event.tags[0],
            vec![
                "r".to_string(),
                "wss://relay.example".to_string(),
                "read".to_string(),
            ]
        );
    }

    #[test]
    fn build_canonicalises_scheme_and_host() {
        let event =
            build_relay_list_event(&[entry("WSS://Relay.Example", RelayMarker::Both)]);
        assert_eq!(
            event.tags,
            vec![vec!["r".to_string(), "wss://relay.example".to_string()]]
        );
    }

    #[test]
    fn build_strips_trailing_slash_on_empty_path_only() {
        let trimmed = build_relay_list_event(&[entry("wss://relay.example/", RelayMarker::Both)]);
        assert_eq!(trimmed.tags[0][1], "wss://relay.example");
        let preserved =
            build_relay_list_event(&[entry("wss://relay.example/nostr/", RelayMarker::Both)]);
        assert_eq!(preserved.tags[0][1], "wss://relay.example/nostr/");
    }

    #[test]
    fn build_drops_non_ws_wss_urls() {
        let event = build_relay_list_event(&[
            entry("http://relay.example", RelayMarker::Both),
            entry("wss://good.example", RelayMarker::Both),
        ]);
        assert_eq!(event.tags.len(), 1);
        assert_eq!(event.tags[0][1], "wss://good.example");
    }

    #[test]
    fn build_drops_malformed_urls() {
        let event = build_relay_list_event(&[
            entry("not a url", RelayMarker::Both),
            entry("wss://", RelayMarker::Both),
            entry("wss://good.example", RelayMarker::Both),
        ]);
        assert_eq!(event.tags.len(), 1);
        assert_eq!(event.tags[0][1], "wss://good.example");
    }

    #[test]
    fn build_with_empty_input_produces_event_with_no_tags() {
        // The builder itself is total — it never panics. The empty-tag
        // guard is the action validator's job, not the builder's.
        let event = build_relay_list_event(&[]);
        assert_eq!(event.kind, 10002);
        assert!(event.tags.is_empty());
    }

    #[test]
    fn build_emits_mixed_markers_in_input_order() {
        let event = build_relay_list_event(&[
            entry("wss://outbox.example", RelayMarker::Write),
            entry("wss://both.example", RelayMarker::Both),
            entry("wss://inbox.example", RelayMarker::Read),
        ]);
        assert_eq!(
            event.tags,
            vec![
                vec![
                    "r".to_string(),
                    "wss://outbox.example".to_string(),
                    "write".to_string(),
                ],
                vec!["r".to_string(), "wss://both.example".to_string()],
                vec![
                    "r".to_string(),
                    "wss://inbox.example".to_string(),
                    "read".to_string(),
                ],
            ],
        );
    }

    // --- action -----------------------------------------------------------

    #[test]
    fn namespace_is_nmp_nip65_publish_relay_list() {
        assert_eq!(
            PublishRelayListAction::NAMESPACE,
            "nmp.nip65.publish_relay_list",
        );
    }

    fn ctx() -> ActionContext {
        ActionContext::default()
    }

    #[test]
    fn start_accepts_a_non_empty_relay_list() {
        let input = PublishRelayListInput {
            relays: vec![entry("wss://relay.example", RelayMarker::Both)],
        };
        assert!(PublishRelayListAction::start(&mut ctx(), input).is_ok());
    }

    #[test]
    fn start_rejects_empty_relay_list() {
        let input = PublishRelayListInput { relays: Vec::new() };
        assert!(matches!(
            PublishRelayListAction::start(&mut ctx(), input),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn start_rejects_input_that_produces_zero_canonical_tags() {
        // All inputs malformed — would build an empty-tag kind:10002 which
        // ingest treats as "clear the cache". The validator must catch
        // this before the actor publishes a destructive event.
        let input = PublishRelayListInput {
            relays: vec![
                entry("not a url", RelayMarker::Both),
                entry("http://nope", RelayMarker::Both),
            ],
        };
        assert!(matches!(
            PublishRelayListAction::start(&mut ctx(), input),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn execute_enqueues_publish_unsigned_event_with_correlation_id() {
        use std::cell::Cell;

        let input = PublishRelayListInput {
            relays: vec![entry("wss://relay.example", RelayMarker::Both)],
        };
        let saw_publish = Cell::new(false);
        let saw_correlation_id = Cell::new(false);
        let saw_kind_10002 = Cell::new(false);
        PublishRelayListAction::execute(input, "test-cid", &|cmd| {
            if let ActorCommand::PublishUnsignedEvent {
                ref event,
                ref correlation_id,
            } = cmd
            {
                saw_publish.set(true);
                if correlation_id.as_deref() == Some("test-cid") {
                    saw_correlation_id.set(true);
                }
                if event.kind == 10002 {
                    saw_kind_10002.set(true);
                }
            }
        })
        .expect("execute must not fail");
        assert!(
            saw_publish.get(),
            "expected PublishUnsignedEvent (NIP-65 Auto-outbox)",
        );
        assert!(
            saw_correlation_id.get(),
            "execute must thread the dispatch correlation_id into the actor command",
        );
        assert!(
            saw_kind_10002.get(),
            "the unsigned event the actor receives must be kind:10002",
        );
    }

    /// Round-trip shape contract: the tag shape the builder produces here
    /// must match what `nmp-core::kernel::nostr::parse_relay_list`
    /// accepts. The parser is `pub(super)` inside `nmp-core`, so we mirror
    /// its core acceptance rules here (tag[0] == "r", url starts with
    /// "wss://", optional third element ∈ {"read","write"}) and assert the
    /// builder output satisfies them. If either side drifts, this test
    /// breaks.
    #[test]
    fn build_event_tags_match_kernel_ingest_shape() {
        let event = build_relay_list_event(&[
            entry("wss://a.example", RelayMarker::Both),
            entry("wss://b.example", RelayMarker::Read),
            entry("wss://c.example", RelayMarker::Write),
        ]);
        for tag in &event.tags {
            assert!(
                tag.len() == 2 || tag.len() == 3,
                "tag must be ['r', url] or ['r', url, marker]; got {:?}",
                tag,
            );
            assert_eq!(tag[0], "r", "NIP-65 tag marker is 'r'");
            assert!(
                tag[1].starts_with("wss://"),
                "ingest parser requires wss:// prefix; got {}",
                tag[1],
            );
            if tag.len() == 3 {
                assert!(
                    tag[2] == "read" || tag[2] == "write",
                    "third element must be 'read' or 'write' (any other value \
                     is parsed as 'both' but would not survive a round trip)",
                );
            }
        }
    }
}
