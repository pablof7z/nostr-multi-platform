//! V-07 — recipient relay injection through the protocol context.

use super::*;

#[test]
fn inject_recipient_relays_preserves_existing_relays_tag() {
    let send = |_: ActorCommand| {};
    let clock = FixedClock(1_700_000_000);
    let signers = LocalSigner;
    let stages = RecordingStages(std::sync::Mutex::new(Vec::new()));
    let recipients = FixedRecipientLookup::with_urls(vec!["wss://from-router.example"]);
    let ctx = ctx_with(&send, &clock, &signers, &stages, &recipients);

    let mut unsigned = unsigned_for(vec![
        vec!["relays".to_string(), "wss://chosen.example".to_string()],
        vec!["p".to_string(), RECIPIENT_HEX.to_string()],
    ]);
    inject_recipient_relays(&ctx, &mut unsigned);
    let relays_tag = unsigned
        .tags
        .iter()
        .find(|t| t.first().map(String::as_str) == Some("relays"))
        .expect("relays tag must be present");
    assert_eq!(
        relays_tag,
        &vec!["relays".to_string(), "wss://chosen.example".to_string()],
        "an explicit non-empty relays tag must be left untouched"
    );
    let relays_count = unsigned
        .tags
        .iter()
        .filter(|t| t.first().map(String::as_str) == Some("relays"))
        .count();
    assert_eq!(relays_count, 1, "must not duplicate the relays tag");
    // And the router must NOT have been consulted — the caller's tag wins.
    assert!(
        recipients.seen.lock().unwrap().is_empty(),
        "router must not be consulted when a filled relays row is present"
    );
}

#[test]
fn inject_recipient_relays_injects_when_tag_absent() {
    let send = |_: ActorCommand| {};
    let clock = FixedClock(1_700_000_000);
    let signers = LocalSigner;
    let stages = RecordingStages(std::sync::Mutex::new(Vec::new()));
    let recipients =
        FixedRecipientLookup::with_urls(vec!["wss://write-a.example", "wss://write-b.example"]);
    let ctx = ctx_with(&send, &clock, &signers, &stages, &recipients);

    let mut unsigned = unsigned_for(vec![vec!["p".to_string(), RECIPIENT_HEX.to_string()]]);
    inject_recipient_relays(&ctx, &mut unsigned);
    let relays_tag = unsigned
        .tags
        .iter()
        .find(|t| t.first().map(String::as_str) == Some("relays"))
        .expect("V-07: actor must inject a relays tag when caller omitted it");
    assert_eq!(
        relays_tag,
        &vec![
            "relays".to_string(),
            "wss://write-a.example".to_string(),
            "wss://write-b.example".to_string(),
        ],
        "must inject every router-resolved URL into the relays row"
    );
    // The router was asked once, for kind:9735 (the zap receipt the LN
    // provider will mint — that's the kind whose publish-direction routes
    // to the recipient's NIP-65 write set).
    assert_eq!(
        *recipients.seen.lock().unwrap(),
        vec![(RECIPIENT_HEX.to_string(), 9735u32)],
        "router must be asked for kind:9735 against the p-tag recipient"
    );
}

#[test]
fn inject_recipient_relays_treats_bare_relays_key_as_absent() {
    // A `["relays"]` row with no URLs is malformed — treat as absent so
    // the injection still fires, AND the malformed row must be discarded.
    let send = |_: ActorCommand| {};
    let clock = FixedClock(1_700_000_000);
    let signers = LocalSigner;
    let stages = RecordingStages(std::sync::Mutex::new(Vec::new()));
    let recipients = FixedRecipientLookup::with_urls(vec!["wss://write.example"]);
    let ctx = ctx_with(&send, &clock, &signers, &stages, &recipients);

    let mut unsigned = unsigned_for(vec![
        vec!["relays".to_string()],
        vec!["p".to_string(), RECIPIENT_HEX.to_string()],
    ]);
    inject_recipient_relays(&ctx, &mut unsigned);
    let relays_rows: Vec<&Vec<String>> = unsigned
        .tags
        .iter()
        .filter(|t| t.first().map(String::as_str) == Some("relays"))
        .collect();
    assert_eq!(
        relays_rows.len(),
        1,
        "must end up with exactly one relays row (the bare one is dropped)"
    );
    assert!(
        relays_rows[0].len() > 1,
        "the surviving relays row must carry the injected URLs: {:?}",
        relays_rows[0]
    );
}

#[test]
fn inject_recipient_relays_falls_back_to_bootstrap_when_p_tag_missing() {
    // Defensive — a builder bug that drops the `p` tag must NOT produce
    // a zap with an empty relays tag. The router resolves the empty
    // recipient against its cold-start AppRelay seed (lane 7) — the test
    // wires that resolution through the `FixedRecipientLookup` adapter
    // (which models the router's lane-7 fallback).
    let send = |_: ActorCommand| {};
    let clock = FixedClock(1_700_000_000);
    let signers = LocalSigner;
    let stages = RecordingStages(std::sync::Mutex::new(Vec::new()));
    let recipients = FixedRecipientLookup::with_urls(vec!["wss://bootstrap.example"]);
    let ctx = ctx_with(&send, &clock, &signers, &stages, &recipients);

    let mut unsigned = unsigned_for(Vec::new());
    inject_recipient_relays(&ctx, &mut unsigned);
    let relays_tag = unsigned
        .tags
        .iter()
        .find(|t| t.first().map(String::as_str) == Some("relays"))
        .expect("must inject a relays tag even when p tag is absent");
    assert_eq!(
        relays_tag,
        &vec!["relays".to_string(), "wss://bootstrap.example".to_string(),],
        "router-resolved URLs (router's own cold-start lane) populate the tag"
    );
    // The router was consulted with an empty recipient pubkey — the LNURL
    // fetcher does not synthesise a fake recipient when the `p` tag is
    // missing; routing decides the fallback (lane 7 in production).
    assert_eq!(
        *recipients.seen.lock().unwrap(),
        vec![(String::new(), 9735u32)],
        "router asked with empty recipient when p tag missing"
    );
}

#[test]
fn inject_recipient_relays_emits_empty_tag_when_router_returns_no_urls() {
    // Documented behaviour from the function doc comment: if the router
    // returns an empty `Vec` (e.g. `RoutingError::Unroutable` — no NIP-65
    // cache hit AND no AppRelay seed), the `relays` tag is still added,
    // empty. The LN provider then falls back to its own default; the
    // contract NIP-57 § "Appendix A" wants the tag PRESENT.
    let send = |_: ActorCommand| {};
    let clock = FixedClock(1_700_000_000);
    let signers = LocalSigner;
    let stages = RecordingStages(std::sync::Mutex::new(Vec::new()));
    let recipients = NoopRecipientRelayLookup;
    let ctx = ctx_with(&send, &clock, &signers, &stages, &recipients);

    let mut unsigned = unsigned_for(vec![vec!["p".to_string(), RECIPIENT_HEX.to_string()]]);
    inject_recipient_relays(&ctx, &mut unsigned);
    let relays_tag = unsigned
        .tags
        .iter()
        .find(|t| t.first().map(String::as_str) == Some("relays"))
        .expect("relays row must be added even with an empty URL set");
    assert_eq!(
        relays_tag,
        &vec!["relays".to_string()],
        "empty router result yields a bare relays row (LN provider falls back)"
    );
}
