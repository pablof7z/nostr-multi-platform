//! Composition seam: build a foreign-NIP event with its owning crate, then
//! route it into a NIP-29 group through the owner-certified `wrap_owned_draft`
//! surface (#2513).
//!
//! This is the path that replaces the deleted per-kind `react_in_group` /
//! `repost_in_group` NIP-29 actions. It demonstrates the kind-blind boundary
//! end to end:
//!
//!   - `nmp-nip25` / `nmp-nip18` own event construction — they build the bare
//!     `kind:7` reaction / `kind:16` repost event (`e` / `p` / `k` tags) with
//!     no group / `h` / routing concern,
//!   - `nmp-nip29` owns only the envelope — `wrap_owned_draft` injects the
//!     `["h", local_id]` group tag and the publish command host-pins it.
//!
//! Neither side knows the other's concern: NIP-25/NIP-18 never name the `h`
//! tag; NIP-29 never names kind:7/16. The app composes them.

use nmp_core::actor::{ActorCommand, PublishCommand};
use nmp_core::publish::PublishRouteClass;
use nmp_nip18::{build_repost_event, RepostAction, KIND_GENERIC_REPOST};
use nmp_nip25::{build_reaction_event, ReactAction, KIND_REACTION};
use nmp_nip29::action::wrap_owned_draft;
use nmp_nip29::GroupId;
use nmp_ownership::{EventOwnershipProvenance, OwnedEventDraft};

const TARGET_ID: &str = "ab";
const AUTHOR_PK: &str = "cd";

fn target_event_id() -> String {
    TARGET_ID.repeat(32)
}

fn target_author() -> String {
    AUTHOR_PK.repeat(32)
}

fn group() -> GroupId {
    GroupId::new("wss://groups.example.com", "room")
}

fn has_tag(tags: &[Vec<String>], name: &str, value: &str) -> bool {
    tags.iter()
        .any(|t| t.len() >= 2 && t[0] == name && t[1] == value)
}

fn assert_artifact(ownership: EventOwnershipProvenance, owner_id: &str, claim_id: &str) {
    assert!(
        ownership
            .artifact
            .is_some_and(|artifact| artifact.owner_id == owner_id && artifact.claim_id == claim_id),
        "missing artifact provenance {owner_id}:{claim_id}: {ownership:?}"
    );
}

fn assert_group_envelope(ownership: EventOwnershipProvenance) {
    assert!(
        ownership.envelopes.iter().any(|envelope| {
            envelope.owner_id == "nmp.nip29" && envelope.claim_id == "nostr.nip29.group_envelope"
        }),
        "missing NIP-29 group envelope provenance: {ownership:?}"
    );
}

#[test]
fn reaction_built_by_nip25_routes_into_group_via_nip29_envelope() {
    // 1. nip25 builds the bare kind:7 reaction — no group concern.
    let reaction = build_reaction_event(&ReactAction {
        target_event_id: target_event_id(),
        reaction: "🔥".to_string(),
        target_author_pubkey: Some(target_author()),
    })
    .expect("nip25 builds a reaction event");
    assert_eq!(reaction.kind, KIND_REACTION, "nip25 owns kind:7");
    assert!(has_tag(&reaction.tags, "e", &target_event_id()));
    assert!(has_tag(&reaction.tags, "p", &target_author()));
    assert!(
        !reaction.tags.iter().any(|t| t.first().is_some_and(|n| n == "h")),
        "nip25 must NOT add the NIP-29 `h` envelope tag"
    );

    let draft = OwnedEventDraft::new(reaction, nmp_nip25::ownership::REACTION_EVENT_PROVENANCE);

    // 2. nip29 wraps it: same kind, content, tags + the injected `h` envelope.
    let group = group();
    let relay = group.host_relay_url.clone();
    let wrapped = wrap_owned_draft(&group, draft, Vec::<String>::new())
        .expect("nmp-nip29 wraps owner-certified reaction draft");
    let cmd = ActorCommand::Publish(PublishCommand::owned_draft_to_relays(
        wrapped,
        vec![relay],
        PublishRouteClass::GroupHostPin,
        Some("test-cid".to_string()),
        None,
    ));
    match cmd {
        ActorCommand::Publish(PublishCommand::OwnedUnsignedEventToRelays {
            event,
            ownership,
            relays,
            route_class,
            ..
        }) => {
            // nip29 preserved the foreign kind verbatim — it never re-kinds.
            assert_eq!(event.kind, KIND_REACTION);
            // nip29 injected ONLY the envelope.
            assert!(
                has_tag(&event.tags, "h", "room"),
                "nip29 must inject the `h` group tag"
            );
            // The owning-NIP tags survive untouched.
            assert!(has_tag(&event.tags, "e", &target_event_id()));
            assert!(has_tag(&event.tags, "p", &target_author()));
            assert_eq!(event.content, "🔥");
            // Host-pinned routing (never the NIP-65 outbox).
            assert_eq!(relays, vec!["wss://groups.example.com".to_string()]);
            assert_eq!(route_class, PublishRouteClass::GroupHostPin);
            assert_artifact(ownership, "nmp.nip25", "nostr.kind.7.reaction");
            assert_group_envelope(ownership);
        }
        other => panic!("expected a host-pinned publish command, got {other:?}"),
    }
}

#[test]
fn repost_built_by_nip18_routes_into_group_via_nip29_envelope() {
    // 1. nip18 builds the bare kind:16 generic repost — no group concern.
    let repost = build_repost_event(&RepostAction {
        target_event_id: target_event_id(),
        target_kind: 9, // a non-kind:1 target → kind:16 generic repost
        target_author_pubkey: Some(target_author()),
        relay_hint: None,
    })
    .expect("nip18 builds a repost event");
    assert_eq!(repost.kind, KIND_GENERIC_REPOST, "nip18 owns kind:16");
    assert!(has_tag(&repost.tags, "e", &target_event_id()));
    assert!(has_tag(&repost.tags, "k", "9"));
    assert!(
        !repost.tags.iter().any(|t| t.first().is_some_and(|n| n == "h")),
        "nip18 must NOT add the NIP-29 `h` envelope tag"
    );

    let draft = OwnedEventDraft::new(
        repost,
        nmp_nip18::ownership::GENERIC_REPOST_EVENT_PROVENANCE,
    );

    // 2. nip29 wraps it: same kind + the injected `h` envelope.
    let group = group();
    let relay = group.host_relay_url.clone();
    let wrapped = wrap_owned_draft(&group, draft, Vec::<String>::new())
        .expect("nmp-nip29 wraps owner-certified repost draft");
    let cmd = ActorCommand::Publish(PublishCommand::owned_draft_to_relays(
        wrapped,
        vec![relay],
        PublishRouteClass::GroupHostPin,
        Some("test-cid".to_string()),
        None,
    ));
    match cmd {
        ActorCommand::Publish(PublishCommand::OwnedUnsignedEventToRelays {
            event,
            ownership,
            relays,
            route_class,
            ..
        }) => {
            assert_eq!(event.kind, KIND_GENERIC_REPOST);
            assert!(
                has_tag(&event.tags, "h", "room"),
                "nip29 must inject the `h` group tag"
            );
            assert!(has_tag(&event.tags, "e", &target_event_id()));
            assert!(has_tag(&event.tags, "k", "9"));
            assert_eq!(relays, vec!["wss://groups.example.com".to_string()]);
            assert_eq!(route_class, PublishRouteClass::GroupHostPin);
            assert_artifact(ownership, "nmp.nip18", "nostr.kind.16.generic_repost");
            assert_group_envelope(ownership);
        }
        other => panic!("expected a host-pinned publish command, got {other:?}"),
    }
}
