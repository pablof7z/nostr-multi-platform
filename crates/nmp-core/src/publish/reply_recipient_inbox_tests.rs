//! chirp#119 regression: a kind:1 reply's `p`-tagged parent author may have
//! no cached kind:10002 (or an otherwise-unresolvable inbox). This must
//! degrade gracefully to the author's own write relays, never fail-closed.
//! Split out of `tests.rs` to keep that file under the 500-LOC hard cap.

use super::*;

#[test]
fn engine_reply_dispatches_to_author_write_relay_when_recipient_inbox_unresolvable() {
    // chirp#119: a kind:1 reply carries an `e`-tag to the parent plus a
    // `p`-tag to the parent author. When the recipient has no cached
    // kind:10002 (or the lookup otherwise yields nothing), `StaticOutbox`'s
    // `p_tag_reads` has no entry for them — mirroring
    // `Nip65OutboxResolver::lookup_kind10002` returning `None` for an
    // uncached recipient. This must NOT abort or shrink the author's own
    // write-relay routing: `OutboxResolver::resolve` is additive-only across
    // the write / local-config / discovery / recipient-inbox lanes (see
    // `nip65_resolver.rs` step 4), so a recipient-inbox miss for one `p` tag
    // must never fail-close a publish that would otherwise deliver fine to
    // the author's own write relays. Regression test for the exact "reply
    // never reaches the relay" symptom reported in chirp#119, even though the
    // resolver code already fails open — see the PR description for the
    // full investigation.
    let mut outbox = StaticOutbox::default();
    outbox
        .author_writes
        .insert("alice".to_string(), vec!["wss://alice-write".to_string()]);
    // Deliberately NOT inserting "bob" into `p_tag_reads` — the recipient's
    // inbox is unresolvable (no cached kind:10002 / a resolution miss).
    let outbox = Arc::new(outbox);
    let dispatcher = Arc::new(ReplayDispatcher::new());
    dispatcher.script("wss://alice-write", vec![RelayAck::ok("wss://alice-write")]);
    let (mut engine, _store, dispatcher) = engine_with(outbox, dispatcher, RetryPolicy::default());

    // kind:1 reply shape: a NIP-10 marked `e`-tag to the parent (routing-
    // irrelevant) plus a `p`-tag to the parent author ("bob"), whose inbox
    // cannot be resolved.
    let mut event = signed_event("ev-reply", "alice", 1, &["bob"]);
    event.unsigned.tags.insert(
        0,
        vec![
            "e".to_string(),
            "parent-id".to_string(),
            String::new(),
            "reply".to_string(),
        ],
    );

    let action = PublishAction::Publish {
        handle: "h-reply".to_string(),
        event,
        target: PublishTarget::Auto,
    };
    engine.start_publish(action, 100, None).unwrap();

    let urls: Vec<String> = dispatcher
        .sent_frames()
        .into_iter()
        .map(|(u, _)| u)
        .collect();
    assert_eq!(
        urls,
        vec!["wss://alice-write".to_string()],
        "a recipient-inbox resolution miss must fail OPEN to the author's own \
         write relays, never fail-closed / NoTargets (chirp#119)"
    );
    assert_eq!(
        engine.snapshot().recent_errors.len(),
        0,
        "the publish must not be treated as NoTargets just because the \
         recipient's inbox could not be resolved"
    );
}
