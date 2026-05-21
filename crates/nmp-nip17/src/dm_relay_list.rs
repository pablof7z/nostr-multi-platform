//! `nmp.nip17.publish_relay_list` — publish the user's own kind:10050 NIP-17
//! DM-relay list.
//!
//! # Why this exists
//!
//! The kernel already INGESTS kind:10050 events (see
//! `nmp-core::kernel::ingest::dm_relay_list`) to populate a `dm_relay_lists`
//! cache keyed by author pubkey. That cache is what the NIP-17 DM send path
//! (`actor::commands::dm`) consults so each kind:1059 envelope is pinned to
//! the receiver's DM-inbox relays per NIP-17 § 2.
//!
//! But without a symmetric **publish** path, every NMP / Chirp user is
//! invisible as a NIP-17 DM recipient — when another client tries to send
//! them a DM, the lookup returns `None` and the sender silently falls back to
//! their *own* Content relays (with a `tracing::warn!`). The recipient simply
//! never receives the message.
//!
//! This action closes that loop: a host publishes its configured DM-inbox
//! relay set as a kind:10050 event under the user's identity, so other clients
//! reading the relay graph can find it.
//!
//! # Tag shape — NIP-17 § 2
//!
//! kind:10050 carries `["relay", <wss-url>]` tags. NOT `["r", ...]` — that is
//! kind:10002 (NIP-65). There is no read/write/both role marker; every entry
//! is a DM-inbox relay. The ingest parser in
//! `nmp-core::kernel::ingest::dm_relay_list::parse_dm_relay_list` is the
//! source of truth and only accepts tags shaped that way; the builder here
//! produces exactly what the parser accepts so a round-trip (publish → ingest)
//! repopulates the same cache.
//!
//! # Routing
//!
//! kind:10050 is a NIP-65 replaceable event (`10000 ≤ kind < 20000`). It
//! should land on the **author's kind:10002 write relays** so other clients
//! pulling the author's relay graph find it. The executor enqueues
//! [`ActorCommand::PublishUnsignedEventToRelays`] with an empty `relays`
//! vec — the actor's dispatch arm falls back to the NIP-65 outbox in that
//! case (see `crates/nmp-core/src/actor/dispatch.rs::PublishUnsignedEventToRelays`).
//!
//! # D7 — `created_at` sentinel
//!
//! The unsigned event is built with `created_at: 0`. The actor re-stamps it
//! from `kernel.now_secs()` before signing; this crate never reads the system
//! clock.

use nmp_core::substrate::{ActionContext, ActionModule, ActionRejection, UnsignedEvent};
use nmp_core::ActorCommand;
use serde::{Deserialize, Serialize};

/// NIP-17 § 2 kind: the DM-relay list — the relays a user wants to receive
/// gift-wrapped DMs at.
const KIND_DM_RELAY_LIST: u32 = 10050;

/// Canonicalize a relay URL the same way `nmp_core::relay::CanonicalRelayUrl`
/// does for ingest, so a publish → ingest round-trip yields exactly the cache
/// entries the host configured.
///
/// Rules (mirroring `CanonicalRelayUrl::parse` — which is `pub(crate)` in
/// `nmp-core` and so cannot be reused across the crate boundary):
///
/// - Trim ASCII whitespace.
/// - Lowercase scheme and authority (host[:port]).
/// - Strip a single trailing `/` only when the path is empty.
/// - Preserve path / query / fragment otherwise.
/// - Return `None` for a missing scheme separator, a non-`ws`/`wss` scheme,
///   or a missing authority — the caller drops the URL (D6 — degrade
///   gracefully rather than emit a kind:10050 with garbage tags).
fn canonicalize_relay_url(raw: &str) -> Option<String> {
    let s = raw.trim();
    let sep = s.find("://")?;
    let scheme = s[..sep].to_ascii_lowercase();
    if scheme != "ws" && scheme != "wss" {
        return None;
    }
    let rest = &s[sep + 3..];
    if rest.is_empty() {
        return None;
    }
    let (authority, path_etc) = if let Some(pos) = rest.find(['/', '?', '#']) {
        (&rest[..pos], &rest[pos..])
    } else {
        (rest, "")
    };
    if authority.is_empty() {
        return None;
    }
    let authority_lower = authority.to_ascii_lowercase();
    let path_etc_norm = if path_etc == "/" { "" } else { path_etc };
    Some(format!("{scheme}://{authority_lower}{path_etc_norm}"))
}

/// Build a NIP-17 kind:10050 DM-relay-list **unsigned** event from an explicit
/// list of relay URLs.
///
/// Per NIP-17 § 2, each entry becomes a `["relay", <url>]` tag — note the
/// `relay` marker, NOT `r` (that is kind:10002, NIP-65). There is no role
/// marker; every entry is a DM-inbox relay.
///
/// URLs are canonicalized (lowercase scheme+host, trailing-`/` stripped on
/// empty path) and deduplicated in first-seen order. URLs that do not parse
/// as `ws://` or `wss://` are dropped — this matches the ingest parser's
/// `wss://` gate so a build → ingest round-trip is stable. NIP-17 § 2
/// recommends `wss://` for DM relays; an explicit `ws://` URL is accepted by
/// the canonicalizer here but will be SKIPPED by `parse_dm_relay_list` on
/// ingest, so callers should configure `wss://`.
///
/// The returned event:
/// - has `kind = 10050`,
/// - has `created_at = 0` — the D7 sentinel; the actor re-stamps it,
/// - has an empty `pubkey` — the actor derives it from the signing keys at
///   sign time (this mirrors NIP-29 actions; the build half of the send is
///   pubkey-agnostic).
pub fn build_dm_relay_list_event(relay_urls: &[String]) -> UnsignedEvent {
    let mut tags: Vec<Vec<String>> = Vec::with_capacity(relay_urls.len());
    let mut seen = std::collections::HashSet::new();
    for raw in relay_urls {
        let Some(canonical) = canonicalize_relay_url(raw) else {
            continue;
        };
        if seen.insert(canonical.clone()) {
            tags.push(vec!["relay".to_string(), canonical]);
        }
    }
    UnsignedEvent {
        // Empty placeholder — the actor re-derives the pubkey from the
        // signing key at sign time (see `ActorCommand::PublishUnsignedEventToRelays`).
        pubkey: String::new(),
        kind: KIND_DM_RELAY_LIST,
        tags,
        content: String::new(),
        // D7 sentinel — the actor re-stamps from `kernel.now_secs()`.
        created_at: 0,
    }
}

/// Wire shape for `nmp.nip17.publish_relay_list` — the JSON a host passes to
/// `nmp_app_dispatch_action`.
///
/// `relays` is the user's DM-inbox relay set. The host is the source of truth
/// here: it reads the user's configured relays from its own UI state and
/// hands them in. Keeping the action stateless (no kernel reads in the
/// executor) is consistent with how the existing NIP-29 / NIP-17 actions
/// build their commands — the executor closure receives only the JSON, the
/// correlation id, and a send callback; it cannot reach into kernel state.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PublishDmRelayListInput {
    /// Relays to advertise as the user's DM-inbox set. Canonicalized and
    /// deduped by the builder; URLs that do not parse as `ws`/`wss` are
    /// dropped.
    pub relays: Vec<String>,
}

/// The `nmp.nip17.publish_relay_list` [`ActionModule`] — a pure shape validator.
///
/// Mirrors `SendDmAction`'s discipline: `start` is a side-effect-free shape
/// check. The actual sign + publish happens on the actor thread (D7).
pub struct PublishDmRelayListAction;

impl ActionModule for PublishDmRelayListAction {
    const NAMESPACE: &'static str = "nmp.nip17.publish_relay_list";
    type Action = PublishDmRelayListInput;

    /// Reject an empty relay set — a kind:10050 with zero `relay` tags is
    /// the canonical "I cleared my DM-inbox list" signal in
    /// `ingest_dm_relay_list`, which REMOVES the cache entry. That is a
    /// destructive operation and should not be reachable via the "publish my
    /// list" verb (a host wanting to clear the list needs its own explicit
    /// verb, which this v1 does not ship).
    fn start(
        _ctx: &mut ActionContext,
        action: Self::Action,
    ) -> Result<(), ActionRejection> {
        if action.relays.is_empty() {
            return Err(ActionRejection::Invalid(
                "empty DM-relay list — refusing to publish a kind:10050 \
                 that would clear the cache for this user"
                    .into(),
            ));
        }
        // Reject input that produces zero canonical tags (every URL was
        // malformed). Reaching the actor with a zero-tag event would emit a
        // valid kind:10050 that clears the cache — the same destructive op
        // the empty-input guard above blocks.
        let event = build_dm_relay_list_event(&action.relays);
        if event.tags.is_empty() {
            return Err(ActionRejection::Invalid(
                "no canonical wss:// / ws:// relay URLs in input".into(),
            ));
        }
        Ok(())
    }
    fn execute(
        action: Self::Action,
        _correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        let event = build_dm_relay_list_event(&action.relays);
        send(ActorCommand::PublishUnsignedEventToRelays {
            event,
            relays: Vec::new(),
        });
        Ok(())
    }
}

/// Executor: build the kind:10050 unsigned event and dispatch
/// [`ActorCommand::PublishUnsignedEventToRelays`] with an EMPTY `relays`
/// vec — kind:10050 is a NIP-65 replaceable event and must land on the
/// author's kind:10002 write relays. An empty `relays` triggers the actor's
/// NIP-65 outbox fallback (`PublishTarget::Auto`).
pub fn publish_dm_relay_list_command(action_json: &str) -> Result<ActorCommand, String> {
    let input: PublishDmRelayListInput =
        serde_json::from_str(action_json).map_err(|e| e.to_string())?;
    let event = build_dm_relay_list_event(&input.relays);
    Ok(ActorCommand::PublishUnsignedEventToRelays {
        event,
        relays: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_produces_kind_10050() {
        let event = build_dm_relay_list_event(&["wss://relay.example".to_string()]);
        assert_eq!(event.kind, 10050);
    }

    #[test]
    fn build_uses_created_at_zero_sentinel() {
        let event = build_dm_relay_list_event(&["wss://relay.example".to_string()]);
        assert_eq!(
            event.created_at, 0,
            "D7: created_at is the 0 sentinel — the actor re-stamps it"
        );
    }

    #[test]
    fn build_leaves_pubkey_empty_for_actor_to_fill() {
        let event = build_dm_relay_list_event(&["wss://relay.example".to_string()]);
        assert!(
            event.pubkey.is_empty(),
            "pubkey is a placeholder — the actor derives it from the signing key"
        );
    }

    #[test]
    fn build_emits_relay_marker_not_r_marker() {
        // NIP-17 § 2 uses ["relay", url] tags. Using ["r", ...] would be a
        // kind:10002 NIP-65 shape; the kernel's parse_dm_relay_list would
        // skip every tag and the round-trip would silently produce an empty
        // cache entry — exactly the leak this test pins.
        let event = build_dm_relay_list_event(&["wss://relay.example".to_string()]);
        assert_eq!(
            event.tags,
            vec![vec!["relay".to_string(), "wss://relay.example".to_string()]],
        );
    }

    #[test]
    fn build_preserves_input_order() {
        let event = build_dm_relay_list_event(&[
            "wss://b.example".to_string(),
            "wss://a.example".to_string(),
            "wss://c.example".to_string(),
        ]);
        let urls: Vec<&String> = event.tags.iter().map(|t| &t[1]).collect();
        assert_eq!(urls, vec!["wss://b.example", "wss://a.example", "wss://c.example"]);
    }

    #[test]
    fn build_dedups_equivalent_urls() {
        // `wss://Relay.Example/` and `wss://relay.example` canonicalize to
        // the same value — only one tag should appear.
        let event = build_dm_relay_list_event(&[
            "wss://Relay.Example/".to_string(),
            "wss://relay.example".to_string(),
        ]);
        assert_eq!(event.tags.len(), 1);
        assert_eq!(event.tags[0], vec!["relay".to_string(), "wss://relay.example".to_string()]);
    }

    #[test]
    fn build_canonicalizes_scheme_and_host() {
        let event = build_dm_relay_list_event(&["WSS://Relay.Example".to_string()]);
        assert_eq!(
            event.tags,
            vec![vec!["relay".to_string(), "wss://relay.example".to_string()]]
        );
    }

    #[test]
    fn build_strips_trailing_slash_on_empty_path_only() {
        // Empty path → trailing slash stripped.
        let trimmed = build_dm_relay_list_event(&["wss://relay.example/".to_string()]);
        assert_eq!(trimmed.tags[0][1], "wss://relay.example");
        // Non-empty path → trailing slash preserved.
        let preserved =
            build_dm_relay_list_event(&["wss://relay.example/nostr/".to_string()]);
        assert_eq!(preserved.tags[0][1], "wss://relay.example/nostr/");
    }

    #[test]
    fn build_drops_non_wss_urls() {
        // `http://` is not a relay scheme; canonicalizer rejects it.
        let event = build_dm_relay_list_event(&[
            "http://relay.example".to_string(),
            "wss://good.example".to_string(),
        ]);
        assert_eq!(event.tags.len(), 1);
        assert_eq!(event.tags[0][1], "wss://good.example");
    }

    #[test]
    fn build_drops_malformed_urls() {
        let event = build_dm_relay_list_event(&[
            "not a url".to_string(),
            "wss://".to_string(),
            "wss://good.example".to_string(),
        ]);
        assert_eq!(event.tags.len(), 1);
        assert_eq!(event.tags[0][1], "wss://good.example");
    }

    #[test]
    fn build_with_empty_input_produces_event_with_no_tags() {
        // The builder itself is total — it never panics. The empty-tag guard
        // is the action validator's job, not the builder's.
        let event = build_dm_relay_list_event(&[]);
        assert_eq!(event.kind, 10050);
        assert!(event.tags.is_empty());
    }

    #[test]
    fn namespace_is_nmp_nip17_publish_relay_list() {
        assert_eq!(
            PublishDmRelayListAction::NAMESPACE,
            "nmp.nip17.publish_relay_list"
        );
    }

    fn ctx() -> ActionContext {
        ActionContext::default()
    }

    #[test]
    fn start_accepts_a_non_empty_relay_list() {
        let input = PublishDmRelayListInput {
            relays: vec!["wss://relay.example".to_string()],
        };
        assert!(PublishDmRelayListAction::start(&mut ctx(), input).is_ok());
    }

    #[test]
    fn start_rejects_empty_relay_list() {
        let input = PublishDmRelayListInput {
            relays: Vec::new(),
        };
        assert!(matches!(
            PublishDmRelayListAction::start(&mut ctx(), input),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn start_rejects_input_that_produces_zero_canonical_tags() {
        // All inputs malformed — would build an empty-tag kind:10050 (which
        // ingest treats as "clear the cache"). The validator must catch this
        // before the actor publishes a destructive event.
        let input = PublishDmRelayListInput {
            relays: vec!["not a url".to_string(), "http://nope".to_string()],
        };
        assert!(matches!(
            PublishDmRelayListAction::start(&mut ctx(), input),
            Err(ActionRejection::Invalid(_))
        ));
    }

    #[test]
    fn command_builds_publish_unsigned_event_to_relays() {
        let body = r#"{"relays":["wss://relay.example"]}"#;
        let cmd = publish_dm_relay_list_command(body).expect("well-formed body");
        match cmd {
            ActorCommand::PublishUnsignedEventToRelays { event, relays } => {
                assert_eq!(event.kind, 10050);
                assert_eq!(
                    event.tags,
                    vec![vec!["relay".to_string(), "wss://relay.example".to_string()]],
                );
                assert_eq!(event.created_at, 0, "D7 sentinel — actor re-stamps");
                assert!(event.pubkey.is_empty(), "actor derives pubkey at sign time");
                assert!(
                    relays.is_empty(),
                    "empty relays → actor's NIP-65 outbox fallback \
                     (kind:10050 is a replaceable event; lands on kind:10002 write relays)"
                );
            }
            other => panic!("expected PublishUnsignedEventToRelays, got {other:?}"),
        }
    }

    #[test]
    fn command_rejects_malformed_json() {
        assert!(publish_dm_relay_list_command("not json").is_err());
    }

    /// Round-trip property: an event the builder produces parses through the
    /// kernel's ingest path back to exactly the canonical URLs the builder
    /// computed. This pins the shape contract between the builder here and
    /// the parser in `nmp-core::kernel::ingest::dm_relay_list`. The parser
    /// itself is `pub(crate)` to nmp-core; we duplicate its core rules here
    /// (tag[0] == "relay", url starts with "wss://") as the structural check
    /// — if either side drifts, this test breaks.
    #[test]
    fn build_event_tags_match_kernel_ingest_shape() {
        let event = build_dm_relay_list_event(&[
            "wss://a.example".to_string(),
            "wss://b.example".to_string(),
        ]);
        // Every tag must be the 2-element ["relay", "wss://..."] shape the
        // kernel's `parse_dm_relay_list` accepts.
        for tag in &event.tags {
            assert_eq!(tag.len(), 2, "tag must be ['relay', <url>]");
            assert_eq!(tag[0], "relay", "tag marker must be 'relay' (not 'r')");
            assert!(
                tag[1].starts_with("wss://"),
                "ingest parser requires wss:// prefix; got {}",
                tag[1],
            );
        }
        let urls: Vec<&String> = event.tags.iter().map(|t| &t[1]).collect();
        assert_eq!(urls, vec!["wss://a.example", "wss://b.example"]);
    }
}
