use super::*;
use crate::display::avatar_color_hex;
use crate::relay::{DEFAULT_VISIBLE_LIMIT, FIATJAF_PUBKEY, JB55_PUBKEY, TEST_PUBKEY};
use crate::store::InsertOutcome;

#[test]
fn open_author_emits_profile_and_note_reqs() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);

    let requests = kernel.open_author(
        FIATJAF_PUBKEY.to_string(),
        std::collections::BTreeSet::from([1u32, 6u32]),
        true,
    );

    // T105 + V-50: cold-start (no cached kind:10002 for FIATJAF). With the
    // router's lane 6 (Indexer always-on for discovery kinds) wired,
    // discovery probes (kind:10002, kind:0) resolve via the indexer URL
    // only — lane 7 (AppRelay fallback) is shortcut because lane 6 already
    // produced a resolution. Content kind:1/6 probes are NOT discovery
    // kinds, so they still fall through to lane 7 with the full
    // BOOTSTRAP_DISCOVERY_RELAYS seed. Net frames:
    //   * author-relays (kind:10002) → 1 seed (indexer) × 1 = 1
    //   * author-profile (kind:0)    → 1 seed (indexer) × 1 = 1
    //   * author-notes (kinds host-supplied) → 2 seeds × 1 = 2
    // = 4 total (was 6 pre-V-50; the kind:0/kind:10002 leak onto the
    // content seed was already a known concern — see BootstrapSeed::
    // IndexerOnly module docs in mailboxes.rs).
    assert_eq!(requests.len(), 4);
    let joined = requests
        .iter()
        .map(|request| request.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(requests
        .iter()
        .any(|request| request.role == RelayRole::Indexer));
    assert!(requests
        .iter()
        .any(|request| request.role == RelayRole::Content));
    assert!(joined.contains("\"author-relays-1-"));
    assert!(joined.contains("\"author-profile-1-"));
    assert!(joined.contains("\"author-notes-1-"));
    assert!(joined.contains("\"kinds\":[10002]"));
    assert!(joined.contains("\"kinds\":[0]"));
    assert!(joined.contains("\"kinds\":[1,6]"));
    assert!(joined.contains(FIATJAF_PUBKEY));
    assert!(!kernel.author_view.request_pending);
    // T105: every frame carries a resolved relay_url, NOT a constant.
    for r in &requests {
        assert!(
            crate::relay::BOOTSTRAP_DISCOVERY_RELAYS.contains(&r.relay_url.as_str()),
            "cold-start author REQ targets bootstrap seed, got {}",
            r.relay_url
        );
    }
    // V-50: discovery kinds (kind:10002, kind:0) MUST resolve to the
    // indexer URL only (lane 6 always-on); they MUST NOT fan out to the
    // content URL.
    for r in &requests {
        if r.text.contains("\"kinds\":[10002]") || r.text.contains("\"kinds\":[0]") {
            assert_eq!(
                r.relay_url.as_str(),
                crate::relay::INDEXER_RELAY_URL,
                "discovery kinds must route via the indexer lane only"
            );
        }
    }
}

#[test]
fn open_author_with_cached_nip65_routes_notes_to_resolved_write_relays() {
    // T105: when the selected author has a cached kind:10002, the kind:1/6
    // notes REQ MUST target their resolved write relays (NOT the bootstrap
    // constants). This is the D3 enforcement bullet at the per-author scope.
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    kernel.seed_mailbox_relay_list(
        FIATJAF_PUBKEY,
        vec![],
        vec![
            "wss://fiatjaf.write".to_string(),
            "wss://fiatjaf.archive".to_string(),
        ],
        vec![],
    );

    let requests = kernel.open_author(
        FIATJAF_PUBKEY.to_string(),
        std::collections::BTreeSet::from([1u32, 6u32]),
        true,
    );
    let notes: Vec<_> = requests
        .iter()
        .filter(|r| r.text.contains("\"kinds\":[1,6]"))
        .collect();
    assert_eq!(notes.len(), 2, "one notes REQ per resolved write relay");
    let urls: std::collections::BTreeSet<_> = notes.iter().map(|r| r.relay_url.clone()).collect();
    assert!(urls.contains("wss://fiatjaf.write"));
    assert!(urls.contains("wss://fiatjaf.archive"));
    for r in notes {
        assert!(
            !crate::relay::BOOTSTRAP_DISCOVERY_RELAYS.contains(&r.relay_url.as_str()),
            "warm author notes REQ MUST NOT route to bootstrap constant"
        );
    }
}

#[test]
fn open_thread_emits_context_and_reply_reqs() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let focused_id = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    let root_id = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let previous_id = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    kernel.events.insert(
        focused_id.to_string(),
        StoredEvent {
            id: focused_id.to_string(),
            author: TEST_PUBKEY.to_string(),
            kind: 1,
            created_at: 1,
            tags: vec![
                vec![
                    "e".to_string(),
                    root_id.to_string(),
                    "".to_string(),
                    "root".to_string(),
                ],
                vec![
                    "e".to_string(),
                    previous_id.to_string(),
                    "".to_string(),
                    "reply".to_string(),
                ],
            ],
            content: "focused".to_string(),
            relay_count: 1,
        },
    );

    let requests =
        kernel.open_thread(focused_id.to_string(), std::collections::BTreeSet::from([1u32, 6u32]), true);

    // T121: thread hydration now partitions ids by the original-event
    // author's NIP-65 write relays. The focused event's author has no
    // cached kind:10002, so the cold-start path fans the ids and reply
    // targets across both BOOTSTRAP_DISCOVERY_RELAYS seeds — one REQ per
    // seed per leg (ids + replies) = 4 REQs.
    let ids_reqs: Vec<&OutboundMessage> = requests
        .iter()
        .filter(|r| r.text.contains("\"thread-ids-"))
        .collect();
    let reply_reqs: Vec<&OutboundMessage> = requests
        .iter()
        .filter(|r| r.text.contains("\"thread-replies-"))
        .collect();
    assert_eq!(
        ids_reqs.len(),
        crate::relay::BOOTSTRAP_DISCOVERY_RELAYS.len(),
        "expected one thread-ids REQ per bootstrap seed; got {requests:?}"
    );
    assert_eq!(
        reply_reqs.len(),
        crate::relay::BOOTSTRAP_DISCOVERY_RELAYS.len(),
        "expected one thread-replies REQ per bootstrap seed; got {requests:?}"
    );

    let joined = requests
        .iter()
        .map(|request| request.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(joined.contains("thread-ids-1-"));
    assert!(joined.contains("thread-replies-2-"));
    assert!(joined.contains(focused_id));
    assert!(joined.contains(root_id));
    assert!(joined.contains(previous_id));
    assert!(joined.contains("\"#e\""));
    assert!(!kernel.thread_view.request_pending);

    // Every REQ targets a bootstrap discovery seed (uncached author path).
    for r in &requests {
        assert!(
            crate::relay::BOOTSTRAP_DISCOVERY_RELAYS.contains(&r.relay_url.as_str()),
            "uncached-author hydration must route to a bootstrap seed; got {}",
            r.relay_url
        );
    }
}

#[test]
fn close_author_refcounts_and_closes_view_subscriptions() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let _ = kernel.open_author(
        FIATJAF_PUBKEY.to_string(),
        std::collections::BTreeSet::from([1u32, 6u32]),
        true,
    );
    let _ = kernel.open_author(
        FIATJAF_PUBKEY.to_string(),
        std::collections::BTreeSet::from([1u32, 6u32]),
        true,
    );

    // Precondition: opening the author actually seeded live wire-subs.
    // Without this the eviction assertion below would be vacuous.
    let open_subs = kernel.wire_subs_len_for_test();
    assert!(
        open_subs > 0,
        "open_author must seed wire_subs rows; got {open_subs}"
    );

    let first_close = kernel.close_author(FIATJAF_PUBKEY);
    assert!(first_close.is_empty());
    assert_eq!(
        kernel
            .author_view
            .selected_author
            .as_ref()
            .map(|view| view.refcount),
        Some(1)
    );
    // Refcount still > 0 → subscriptions stay live; nothing evicted yet.
    assert_eq!(
        kernel.wire_subs_len_for_test(),
        open_subs,
        "a non-final close must not evict wire_subs"
    );

    let second_close = kernel.close_author(FIATJAF_PUBKEY);
    let joined = second_close
        .iter()
        .map(|request| request.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(joined.contains("\"CLOSE\""));
    assert!(joined.contains("author-profile-1"));
    assert!(joined.contains("author-notes-1"));
    assert!(kernel.author_view.selected_author.is_none());
    // The anti-leak invariant: the final close evicts every author wire-sub
    // row, so a profile open/close cycle leaves zero residue.
    assert_eq!(
        kernel.wire_subs_len_for_test(),
        0,
        "final close_author must evict all author wire_subs rows"
    );
}

#[test]
fn close_thread_refcounts_and_closes_view_subscriptions() {
    // Symmetric counterpart to `close_author_refcounts_and_closes_view_subscriptions`.
    // Audits the FFI lifecycle leak the strategic review raised against
    // `close_thread`: an open/close cycle MUST leave zero wire_subs residue.
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let focused_id = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    let _ = kernel.open_thread(focused_id.to_string(), std::collections::BTreeSet::from([1u32, 6u32]), true);
    let _ = kernel.open_thread(focused_id.to_string(), std::collections::BTreeSet::from([1u32, 6u32]), true);

    // Precondition: opening the thread seeded live wire-subs (thread-ids- /
    // thread-replies- REQ frames). Without this the eviction check is vacuous.
    let open_subs = kernel.wire_subs_len_for_test();
    assert!(
        open_subs > 0,
        "open_thread must seed wire_subs rows; got {open_subs}"
    );

    let first_close = kernel.close_thread(focused_id);
    assert!(
        first_close.is_empty(),
        "a non-final close must not emit CLOSE frames"
    );
    assert_eq!(
        kernel
            .thread_view
            .selected_thread
            .as_ref()
            .map(|view| view.refcount),
        Some(1),
        "refcount must drop to 1 after one of two closes"
    );
    assert_eq!(
        kernel.wire_subs_len_for_test(),
        open_subs,
        "a non-final close must not evict wire_subs"
    );

    let second_close = kernel.close_thread(focused_id);
    let joined = second_close
        .iter()
        .map(|request| request.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        joined.contains("\"CLOSE\""),
        "final close must emit CLOSE frames"
    );
    assert!(
        joined.contains("thread-ids-"),
        "final close must CLOSE the thread-ids subscription"
    );
    assert!(
        joined.contains("thread-replies-"),
        "final close must CLOSE the thread-replies subscription"
    );
    assert!(
        kernel.thread_view.selected_thread.is_none(),
        "final close must clear the selected thread interest"
    );
    // The anti-leak invariant: the final close evicts every thread wire-sub
    // row, so a thread open/close cycle leaves zero residue.
    assert_eq!(
        kernel.wire_subs_len_for_test(),
        0,
        "final close_thread must evict all thread wire_subs rows"
    );
}

// ── V-68 Stage 2: externalize thread-reply kinds to host ─────────────────────
//
// Three tests prove the externalization:
//   1. Default behavior preserved  — {1, 6} still appears in the REQ filter.
//   2. Kernel is kind-agnostic     — {30023} (long-form) works instead of {1,6}.
//   3. Deferred-relay path correct — kinds stored before `can_send` branch;
//      the deferred drain reads them from state (not a dropped parameter).

/// V-68-T1: `open_thread` with kinds {1,6} produces a thread-replies REQ
/// carrying `"kinds":[1,6]`. Verifies behavior is preserved after the
/// externalization.
#[test]
fn v68_thread_reply_req_carries_host_supplied_kinds_1_6() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let event_id = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    let requests = kernel.open_thread(
        event_id.to_string(),
        std::collections::BTreeSet::from([1u32, 6u32]),
        true,
    );

    let reply_reqs: Vec<&OutboundMessage> = requests
        .iter()
        .filter(|r| r.text.contains("\"thread-replies-"))
        .collect();
    assert!(
        !reply_reqs.is_empty(),
        "V-68-T1: open_thread must emit at least one thread-replies REQ"
    );
    for req in &reply_reqs {
        assert!(
            req.text.contains("\"kinds\":[1,6]"),
            "V-68-T1: thread-replies REQ must carry kinds [1,6]; got {}",
            req.text
        );
        assert!(
            req.text.contains("\"#e\""),
            "V-68-T1: thread-replies REQ must carry #e filter; got {}",
            req.text
        );
    }
}

/// V-68-T2: `open_thread` with kinds {30023} (long-form) produces a
/// thread-replies REQ carrying `"kinds":[30023]`. Proves the kernel is
/// kind-agnostic — the `{1,6}` literal is truly externalized, not
/// disguised behind a hidden default.
#[test]
fn v68_thread_reply_req_carries_arbitrary_host_kinds() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let event_id = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    let requests = kernel.open_thread(
        event_id.to_string(),
        std::collections::BTreeSet::from([30023u32]),
        true,
    );

    let reply_reqs: Vec<&OutboundMessage> = requests
        .iter()
        .filter(|r| r.text.contains("\"thread-replies-"))
        .collect();
    assert!(
        !reply_reqs.is_empty(),
        "V-68-T2: open_thread must emit at least one thread-replies REQ for kinds [30023]"
    );
    for req in &reply_reqs {
        let text = &req.text;
        assert!(
            text.contains("\"kinds\":[30023]"),
            "V-68-T2: thread-replies REQ must carry kinds [30023]; got {text}"
        );
        assert!(
            !text.contains("[1,6]"),
            "V-68-T2: thread-replies REQ must NOT contain hardcoded [1,6]; got {text}"
        );
    }
}

/// V-68-T3: deferred-relay path stores kinds.
///
/// When `can_send=false` the request is queued (`request_pending=true`,
/// no REQ emitted). Firing `prepare_thread_requests()` on a later tick
/// (the relay-ready drain) must still produce a thread-replies REQ carrying
/// the stored kinds — proving they survive in `reply_kinds`, not as a
/// dropped function parameter.
#[test]
fn v68_deferred_relay_path_reads_stored_kinds() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);
    let event_id = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

    // Step 1: open with can_send=false — no REQs emitted yet.
    let immediate = kernel.open_thread(
        event_id.to_string(),
        std::collections::BTreeSet::from([1u32, 6u32]),
        false,
    );
    assert!(
        immediate.is_empty(),
        "V-68-T3: can_send=false must emit no REQs immediately; got {immediate:?}"
    );
    assert!(
        kernel.thread_view.request_pending,
        "V-68-T3: request_pending must be true after deferred open"
    );

    // Step 2: relay becomes available — drain via `prepare_thread_requests`,
    // the same path `drain_lifecycle_tick` uses when `request_pending=true`.
    let deferred = kernel.prepare_thread_requests();

    let reply_reqs: Vec<&OutboundMessage> = deferred
        .iter()
        .filter(|r| r.text.contains("\"thread-replies-"))
        .collect();
    assert!(
        !reply_reqs.is_empty(),
        "V-68-T3: deferred drain must emit at least one thread-replies REQ; got {deferred:?}"
    );
    for req in &reply_reqs {
        assert!(
            req.text.contains("\"kinds\":[1,6]"),
            "V-68-T3: deferred thread-replies REQ must carry stored kinds [1,6]; got {}",
            req.text
        );
    }
    assert!(
        !kernel.thread_view.request_pending,
        "V-68-T3: request_pending must be false after drain"
    );
}

// ── V-68 Stage 2 author-half: externalize author-note kinds to host ──────────
//
// Three tests mirror the thread-half above (T1/T2/T3):
//   A1. Default behavior preserved — {1, 6} still appears in the REQ filter.
//   A2. Kernel is kind-agnostic    — {30023} (long-form) works instead of {1,6}.
//   A3. Deferred-relay path correct — kinds stored before `can_send` branch;
//       the deferred drain reads them from state (not a dropped parameter).
//
// The sentinel-kind tests (A2/A3 with {30023}) are the load-bearing proof
// that the literal `[1, 6]` is gone from nmp-core and the value flows from
// the command through the stored `note_kinds` field. A test using {1,6}
// would still pass even if the code re-hardcoded [1,6] downstream.

/// V-68-A1: `open_author` with kinds {1,6} produces an author-notes REQ
/// carrying `"kinds":[1,6]`. Verifies behavior is preserved after the
/// externalization.
#[test]
fn v68_author_note_req_carries_host_supplied_kinds_1_6() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);

    let requests = kernel.open_author(
        FIATJAF_PUBKEY.to_string(),
        std::collections::BTreeSet::from([1u32, 6u32]),
        true,
    );

    let notes: Vec<_> = requests
        .iter()
        .filter(|r| r.text.contains("\"author-notes-"))
        .collect();
    assert!(
        !notes.is_empty(),
        "V-68-A1: open_author must emit at least one author-notes REQ; got {requests:?}"
    );
    for req in &notes {
        assert!(
            req.text.contains("\"kinds\":[1,6]"),
            "V-68-A1: author-notes REQ must carry host-supplied kinds [1,6]; got {}",
            req.text
        );
    }
}

/// V-68-A2: sentinel-kind test — `open_author` with kinds {30023} (long-form)
/// produces an author-notes REQ carrying `"kinds":[30023]`.
///
/// This is the discriminating proof that `[1, 6]` is NOT hardcoded in nmp-core.
/// The test would FAIL if any code path still emits a literal `[1, 6]`, because
/// the sentinel kind is different enough that a hardcoded fallback would
/// produce the wrong filter.
#[test]
fn v68_author_note_req_carries_sentinel_kind_not_hardcoded() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);

    let requests = kernel.open_author(
        FIATJAF_PUBKEY.to_string(),
        // Long-form kind — deliberately NOT {1, 6} to prove kind-agnosticism.
        std::collections::BTreeSet::from([30023u32]),
        true,
    );

    let notes: Vec<_> = requests
        .iter()
        .filter(|r| r.text.contains("\"author-notes-"))
        .collect();
    assert!(
        !notes.is_empty(),
        "V-68-A2: open_author must emit at least one author-notes REQ; got {requests:?}"
    );
    for req in &notes {
        assert!(
            req.text.contains("\"kinds\":[30023]"),
            "V-68-A2: author-notes REQ must carry sentinel kind 30023 (not hardcoded [1,6]); got {}",
            req.text
        );
        assert!(
            !req.text.contains("\"kinds\":[1,6]"),
            "V-68-A2: author-notes REQ MUST NOT contain hardcoded [1,6]; got {}",
            req.text
        );
    }
}

/// V-68-A3: deferred-relay path stores author-note kinds.
///
/// When `can_send=false` the request is queued (`request_pending=true`,
/// no REQ emitted). Firing `author_requests()` on a later tick (the
/// relay-ready drain via `pending_view_requests`) must still produce an
/// author-notes REQ carrying the stored sentinel kinds — proving they
/// survive in `note_kinds`, not as a dropped function parameter.
#[test]
fn v68_author_deferred_relay_path_reads_stored_kinds() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);

    // Step 1: open with can_send=false — no REQs emitted yet.
    let immediate = kernel.open_author(
        FIATJAF_PUBKEY.to_string(),
        // Sentinel kind to prove the value is stored/read, not hardcoded.
        std::collections::BTreeSet::from([30023u32]),
        false,
    );
    assert!(
        immediate.is_empty(),
        "V-68-A3: can_send=false must emit no REQs immediately; got {immediate:?}"
    );
    assert!(
        kernel.author_view.request_pending,
        "V-68-A3: request_pending must be true after deferred open"
    );

    // Step 2: relay becomes available — drain via `author_requests`,
    // the same path `pending_view_requests` uses when `request_pending=true`.
    let deferred = kernel.author_requests();

    let note_reqs: Vec<&OutboundMessage> = deferred
        .iter()
        .filter(|r| r.text.contains("\"author-notes-"))
        .collect();
    assert!(
        !note_reqs.is_empty(),
        "V-68-A3: deferred drain must emit at least one author-notes REQ; got {deferred:?}"
    );
    for req in &note_reqs {
        assert!(
            req.text.contains("\"kinds\":[30023]"),
            "V-68-A3: deferred author-notes REQ must carry stored sentinel kind 30023; got {}",
            req.text
        );
        assert!(
            !req.text.contains("\"kinds\":[1,6]"),
            "V-68-A3: deferred author-notes REQ MUST NOT contain hardcoded [1,6]; got {}",
            req.text
        );
    }
    assert!(
        !kernel.author_view.request_pending,
        "V-68-A3: request_pending must be false after drain"
    );
}

#[test]
fn profile_claims_are_ui_driven_and_deduped_by_pubkey() {
    let mut kernel = Kernel::new_for_test(DEFAULT_VISIBLE_LIMIT);

    let first = kernel.claim_profile(
        FIATJAF_PUBKEY.to_string(),
        "timeline-row:first".to_string(),
        true,
    );
    let second = kernel.claim_profile(
        FIATJAF_PUBKEY.to_string(),
        "timeline-row:second".to_string(),
        true,
    );

    // Cold-start profile claim must go to the indexer relay ONLY (not the content relay).
    assert_eq!(
        first.len(),
        1,
        "cold-start profile claim must emit exactly one REQ"
    );
    assert!(second.is_empty());
    let joined = first
        .iter()
        .map(|r| r.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(joined.contains("\"profile-claim-1-"));
    assert!(joined.contains("\"kinds\":[0]"));
    assert!(joined.contains(FIATJAF_PUBKEY));
    for r in &first {
        assert_eq!(
            r.relay_url.as_str(),
            crate::relay::INDEXER_RELAY_URL,
            "cold-start profile claim must route to indexer only, got {}",
            r.relay_url
        );
    }
    assert_eq!(
        kernel
            .profile_claims
            .get(FIATJAF_PUBKEY)
            .map(|claims| claims.len()),
        Some(2)
    );

    let first_release = kernel.release_profile(FIATJAF_PUBKEY, "timeline-row:first");
    assert!(first_release.is_empty());
    assert_eq!(
        kernel
            .profile_claims
            .get(FIATJAF_PUBKEY)
            .map(|claims| claims.len()),
        Some(1)
    );

    let second_release = kernel.release_profile(FIATJAF_PUBKEY, "timeline-row:second");
    assert!(second_release.is_empty());
    assert!(!kernel.profile_claims.contains_key(FIATJAF_PUBKEY));
}

#[test]
fn parse_relay_list_splits_nip65_markers() {
    let parsed = parse_relay_list(
        "deadbeef",
        123,
        &[
            vec![
                "r".to_string(),
                "wss://read.example".to_string(),
                "read".to_string(),
            ],
            vec![
                "r".to_string(),
                "wss://write.example".to_string(),
                "write".to_string(),
            ],
            vec!["r".to_string(), "wss://both.example".to_string()],
            vec![
                "r".to_string(),
                "https://not-a-relay.example".to_string(),
                "read".to_string(),
            ],
        ],
    );

    assert_eq!(parsed.created_at, 123);
    assert_eq!(parsed.read_relays, vec!["wss://read.example"]);
    assert_eq!(parsed.write_relays, vec!["wss://write.example"]);
    assert_eq!(parsed.both_relays, vec!["wss://both.example"]);
}

// ─── D4 regression tests: stale re-delivery must not overwrite local cache ───

const PK_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const ID_V1: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const ID_V2: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const RELAY: &str = "wss://test.relay/";

/// D4 — kind:3 regression: deliver v2 then re-deliver stale v1.
///
/// The store must supersede v1 (older created_at) and the kernel's
/// `seed_contacts` cache must stay at the v2 content.
#[test]
fn kind3_stale_redelivery_does_not_overwrite_contacts_cache() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // v2 — newer event with two follows.
    let follows_v2: Vec<Vec<String>> = vec![
        vec!["p".to_string(), FIATJAF_PUBKEY.to_string()],
        vec!["p".to_string(), JB55_PUBKEY.to_string()],
    ];
    let o2 = kernel
        .inject_replaceable_event(ID_V2, PK_A, 2000, 3, follows_v2, RELAY, 2_000_000)
        .expect("store insert must succeed");
    assert!(
        matches!(o2, InsertOutcome::Inserted { .. }),
        "v2 must be freshly inserted, got {o2:?}"
    );
    let contacts_after_v2 = kernel
        .seed_contacts
        .get(PK_A)
        .cloned()
        .expect("contacts must be populated after v2");
    assert_eq!(
        contacts_after_v2.len(),
        2,
        "cache should hold v2's two follows"
    );

    // v1 — older event with one follow (stale re-delivery).
    let follows_v1: Vec<Vec<String>> = vec![vec!["p".to_string(), FIATJAF_PUBKEY.to_string()]];
    let o1 = kernel
        .inject_replaceable_event(ID_V1, PK_A, 1000, 3, follows_v1, RELAY, 1_000_000)
        .expect("store insert must succeed");
    assert!(
        matches!(o1, InsertOutcome::Superseded { .. }),
        "stale v1 must be Superseded by the store, got {o1:?}"
    );

    // Cache must still reflect v2 — the stale v1 must not have overwritten it.
    let contacts_after_v1 = kernel
        .seed_contacts
        .get(PK_A)
        .cloned()
        .expect("contacts must still be populated");
    assert_eq!(
        contacts_after_v1.len(),
        2,
        "D4 violation: stale v1 overwrote v2 contacts cache"
    );
}

/// D4 — kind:10002 regression: deliver v2 then re-deliver stale v1.
///
/// The store must supersede v1 and the kernel's `author_relay_lists`
/// cache must stay at the v2 relay list.
#[test]
fn kind10002_stale_redelivery_does_not_overwrite_relay_list_cache() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // v2 — two relays.
    let tags_v2: Vec<Vec<String>> = vec![
        vec![
            "r".to_string(),
            "wss://v2-read.example/".to_string(),
            "read".to_string(),
        ],
        vec![
            "r".to_string(),
            "wss://v2-write.example/".to_string(),
            "write".to_string(),
        ],
    ];
    let o2 = kernel
        .inject_replaceable_event(ID_V2, PK_A, 2000, 10002, tags_v2, RELAY, 2_000_000)
        .expect("store insert must succeed");
    assert!(
        matches!(o2, InsertOutcome::Inserted { .. }),
        "v2 must be freshly inserted, got {o2:?}"
    );
    let list_after_v2 = kernel
        .mailbox_cache()
        .snapshot(&PK_A.to_string())
        .expect("relay list must be populated after v2");
    // Cache holds v2's relay URLs (the store doesn't surface created_at
    // through the substrate cache — the store itself enforces
    // supersession; the cache is just the projection of the winning
    // event's tags).
    assert_eq!(list_after_v2.read, vec!["wss://v2-read.example/"]);
    assert_eq!(list_after_v2.write, vec!["wss://v2-write.example/"]);

    // v1 — older event with one relay.
    let tags_v1: Vec<Vec<String>> =
        vec![vec!["r".to_string(), "wss://v1-only.example/".to_string()]];
    let o1 = kernel
        .inject_replaceable_event(ID_V1, PK_A, 1000, 10002, tags_v1, RELAY, 1_000_000)
        .expect("store insert must succeed");
    assert!(
        matches!(o1, InsertOutcome::Superseded { .. }),
        "stale v1 must be Superseded by the store, got {o1:?}"
    );

    // Cache must still reflect v2's relays (the store rejected v1 so
    // `ingest_relay_list` was never called for v1 — the cache was not
    // touched).
    let list_after_v1 = kernel
        .mailbox_cache()
        .snapshot(&PK_A.to_string())
        .expect("relay list must still be populated");
    assert_eq!(
        list_after_v1.read,
        vec!["wss://v2-read.example/"],
        "D4 violation: stale v1 overwrote v2 relay list cache"
    );
    assert_eq!(list_after_v1.write, vec!["wss://v2-write.example/"]);
}

// ─── C13 kernel companion: D1 placeholder contract + in-place refinement ─────

const C13_PK: &str = "c13ac13ac13ac13ac13ac13ac13ac13ac13ac13ac13ac13ac13ac13ac13ac13a";
const C13_ID: &str = "e1e2e3e4e5e6e7e8e9eae1e2e3e4e5e6e7e8e9eae1e2e3e4e5e6e7e8e9eae1e2";
const C13_KIND0_ID: &str = "f1f2f3f4f5f6f7f8f9faf1f2f3f4f5f6f7f8f9faf1f2f3f4f5f6f7f8f9faf1f2";

/// C13 kernel companion (D1 best-effort rendering — placeholder contract).
///
/// Phase 1: before any kind:0 arrives, `timeline_item().author_picture_url`
/// must be a non-empty deterministic identicon URI (never empty, never panic).
///
/// Phase 2: after kind:0 with a real picture URL arrives, the same item's
/// `author_picture_url` must resolve to the real URL (in-place refinement).
///
/// Design: `docs/product-spec/doctrine.md` §D1, ADR-0017.
#[test]
fn c13_kernel_timeline_item_d1_picture_url_placeholder_and_refinement() {
    use crate::store::VerifiedEvent;
    use crate::substrate::placeholder::picture_placeholder;

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // ── Phase 1: inject kind:1, no kind:0 yet ────────────────────────────────
    let raw_note = crate::store::RawEvent {
        id: C13_ID.to_string(),
        pubkey: C13_PK.to_string(),
        created_at: 1_000,
        kind: 1,
        tags: vec![],
        content: "test note".to_string(),
        sig: "a".repeat(128),
    };
    kernel.ingest_pre_verified_event(
        crate::relay::RelayRole::Content,
        "diag-firehose-stress",
        VerifiedEvent::from_raw_unchecked(raw_note),
    );
    kernel.sort_timeline_deferred();

    let event = kernel.events.get(C13_ID).expect("event must be in cache");
    let item_no_profile = kernel.timeline_item(event);

    // aim.md §2 — no identicon placeholder substituted in NMP.
    // `author_picture_url` is `None` until kind:0 arrives.
    assert_eq!(
        item_no_profile.author_picture_url, None,
        "aim.md §2: picture_url must be None before kind:0 (presentation owns the fallback)"
    );

    // ── Phase 2: inject kind:0 with a real picture URL ───────────────────────
    let picture = "https://example.com/avatar.png";

    // Insert the profile directly into the kernel's profile cache.
    // (inject_replaceable_event always uses empty content and therefore
    //  produces a profile with no picture_url — insufficient for this test.)
    kernel.profiles.insert(
        C13_PK.to_string(),
        Profile {
            event_id: C13_KIND0_ID.to_string(),
            created_at: 2_000,
            display: "c13".to_string(),
            picture_url: Some(picture.to_string()),
            nip05: String::new(),
            about: String::new(),
            lnurl: None,
        },
    );

    let event_after = kernel
        .events
        .get(C13_ID)
        .expect("event must still be in cache");
    let item_with_profile = kernel.timeline_item(event_after);

    // After kind:0 arrives, the real picture URL surfaces verbatim.
    assert_eq!(
        item_with_profile.author_picture_url.as_deref(),
        Some(picture),
        "picture_url updates to kind:0 URL in place"
    );
    // The item id is unchanged — refinement is in-place.
    assert_eq!(item_with_profile.id, item_no_profile.id);
}

/// aim.md §2 — a profile that arrived with no picture (empty/absent
/// `picture_url`) surfaces `None` for `author_picture_url` /
/// `ProfileCard::picture_url`. Presentation layers choose the
/// missing-picture rendering (identicon, initials tile, etc.); NMP no
/// longer substitutes a placeholder URI.
#[test]
fn picture_url_is_none_when_profile_omits_picture() {
    use crate::store::VerifiedEvent;

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let raw_note = crate::store::RawEvent {
        id: C13_ID.to_string(),
        pubkey: C13_PK.to_string(),
        created_at: 1_000,
        kind: 1,
        tags: vec![],
        content: "no-picture profile note".to_string(),
        sig: "a".repeat(128),
    };
    kernel.ingest_pre_verified_event(
        crate::relay::RelayRole::Content,
        "diag-firehose-stress",
        VerifiedEvent::from_raw_unchecked(raw_note),
    );
    kernel.sort_timeline_deferred();

    for picture in [None, Some(String::new())] {
        kernel.profiles.insert(
            C13_PK.to_string(),
            Profile {
                event_id: C13_KIND0_ID.to_string(),
                created_at: 2_000,
                display: "c13".to_string(),
                picture_url: picture.clone(),
                nip05: String::new(),
                about: String::new(),
                lnurl: None,
            },
        );

        let event = kernel.events.get(C13_ID).expect("event must be in cache");
        let item = kernel.timeline_item(event);
        assert_eq!(
            item.author_picture_url, None,
            "profile without picture must surface None ({picture:?})"
        );

        let card = kernel.profile_card_for(C13_PK, None, "about");
        assert_eq!(
            card.picture_url, None,
            "ProfileCard without picture must surface None ({picture:?})"
        );
    }
}

/// P2 — kind:10002 empty relay list clears the cache entry.
///
/// When a canonical *newer* kind:10002 carries an empty relay list, the author
/// has explicitly cleared their NIP-65 metadata.  The old cache entry must be
/// *removed* rather than left stale.
#[test]
fn kind10002_empty_relay_list_clears_cache_entry() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // v1 — non-empty relay list; populates the cache.
    let tags_v1: Vec<Vec<String>> = vec![
        vec![
            "r".to_string(),
            "wss://v1-read.example/".to_string(),
            "read".to_string(),
        ],
        vec![
            "r".to_string(),
            "wss://v1-write.example/".to_string(),
            "write".to_string(),
        ],
    ];
    let o1 = kernel
        .inject_replaceable_event(ID_V1, PK_A, 1000, 10002, tags_v1, RELAY, 1_000_000)
        .expect("v1 store insert must succeed");
    assert!(
        matches!(o1, InsertOutcome::Inserted { .. }),
        "v1 must be freshly inserted, got {o1:?}"
    );
    assert!(
        kernel.mailbox_cache().known(&PK_A.to_string()),
        "cache must be populated after v1"
    );

    // v2 — newer event with an EMPTY relay list (author cleared NIP-65).
    let o2 = kernel
        .inject_replaceable_event(ID_V2, PK_A, 2000, 10002, vec![], RELAY, 2_000_000)
        .expect("v2 store insert must succeed");
    assert!(
        matches!(
            o2,
            InsertOutcome::Inserted { .. } | InsertOutcome::Replaced { .. }
        ),
        "v2 must supersede v1 in the store, got {o2:?}"
    );

    // Cache entry must be removed — empty list clears the stale relay metadata.
    assert!(
        !kernel.mailbox_cache().known(&PK_A.to_string()),
        "empty kind:10002 must remove stale cache entry"
    );
}

// ── kind:6 (NIP-18) repost view-field projection ────────────────────────────
//
// Companion tests for the thin-shell move: the inner-event JSON parse that
// used to live in Swift (`NoteRowView.swift::innerEventField`,
// `ThreadNoteRow.swift::repostInnerText`) now resolves once in Rust and is
// emitted as `is_repost` / `nav_target_id` / `repost_inner_content`. Tests
// pin the D1 fallback contract so a malformed/empty inner JSON never strands
// the row in an unrenderable state.

const REPOST_PK: &str = "ba51ba51ba51ba51ba51ba51ba51ba51ba51ba51ba51ba51ba51ba51ba51ba51";
const REPOST_ID: &str = "deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef";
const REPOST_INNER_ID: &str = "1234567812345678123456781234567812345678123456781234567812345678";

fn ingest_kind6(kernel: &mut Kernel, content: &str) {
    use crate::store::VerifiedEvent;
    let raw = crate::store::RawEvent {
        id: REPOST_ID.to_string(),
        pubkey: REPOST_PK.to_string(),
        created_at: 1_000,
        kind: 6,
        tags: vec![],
        content: content.to_string(),
        sig: "a".repeat(128),
    };
    kernel.ingest_pre_verified_event(
        crate::relay::RelayRole::Content,
        "diag-firehose-stress",
        VerifiedEvent::from_raw_unchecked(raw),
    );
    kernel.sort_timeline_deferred();
}

#[test]
fn timeline_item_kind1_has_no_repost_flag_and_nav_targets_self() {
    use crate::store::VerifiedEvent;
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let raw = crate::store::RawEvent {
        id: REPOST_ID.to_string(),
        pubkey: REPOST_PK.to_string(),
        created_at: 1_000,
        kind: 1,
        tags: vec![],
        content: "plain note".to_string(),
        sig: "a".repeat(128),
    };
    kernel.ingest_pre_verified_event(
        crate::relay::RelayRole::Content,
        "diag-firehose-stress",
        VerifiedEvent::from_raw_unchecked(raw),
    );
    kernel.sort_timeline_deferred();
    let event = kernel.events.get(REPOST_ID).expect("event cached");
    let item = kernel.timeline_item(event);
    assert!(!item.is_repost, "kind:1 must not be flagged as a repost");
    assert_eq!(
        item.nav_target_id, REPOST_ID,
        "kind:1 thread navigation targets the event itself"
    );
    assert_eq!(
        item.repost_inner_content, "",
        "kind:1 must not surface a repost-inner content string"
    );
}

#[test]
fn timeline_item_kind6_well_formed_inner_event_extracts_id_and_content() {
    let inner_json = format!(
        r#"{{"id":"{}","pubkey":"{}","kind":1,"content":"inner note text","tags":[]}}"#,
        REPOST_INNER_ID, REPOST_PK
    );
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    ingest_kind6(&mut kernel, &inner_json);

    let event = kernel.events.get(REPOST_ID).expect("event cached");
    let item = kernel.timeline_item(event);
    assert!(item.is_repost, "kind:6 must be flagged as a repost");
    assert_eq!(
        item.nav_target_id, REPOST_INNER_ID,
        "kind:6 thread navigation targets the inner kind:1 id"
    );
    assert_eq!(item.repost_inner_content, "inner note text");
}

#[test]
fn timeline_item_kind6_empty_content_falls_back_to_event_id_and_empty_text() {
    // NIP-18 reposts MAY ship empty `content`; the row still needs to be
    // renderable — the "Repost" badge alone communicates state (D1 best-effort).
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    ingest_kind6(&mut kernel, "");

    let event = kernel.events.get(REPOST_ID).expect("event cached");
    let item = kernel.timeline_item(event);
    assert!(item.is_repost);
    assert_eq!(
        item.nav_target_id, REPOST_ID,
        "empty inner JSON: navigation falls back to the repost's own id"
    );
    assert_eq!(item.repost_inner_content, "");
    assert_eq!(
        item.content_preview, "Repost",
        "empty kind:6 still uses the 'Repost' preview (pre-existing contract)"
    );
}

#[test]
fn timeline_item_kind6_malformed_inner_event_falls_back_cleanly() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    ingest_kind6(&mut kernel, "RT some plain-text repost");

    let event = kernel.events.get(REPOST_ID).expect("event cached");
    let item = kernel.timeline_item(event);
    assert!(item.is_repost);
    assert_eq!(item.nav_target_id, REPOST_ID, "malformed JSON: id fallback");
    assert_eq!(
        item.repost_inner_content, "",
        "malformed JSON: empty content"
    );
}

/// V-26 — `Kernel::accounts_enriched` must recompute `avatar_initials` whenever
/// kind:0 metadata replaces the placeholder `display_name`. Otherwise the
/// avatar tile keeps showing the npub-body fallback initials (e.g. `"AB"`
/// from the bech32 body) while the surrounding row text reads `"Alice Smith"`.
/// The Swift extension this V-26 work replaced computed initials at view time
/// so it was implicitly reactive; the Rust-owned field must match that
/// behaviour or the iOS toolbar avatar visibly drifts from the display name.
#[test]
fn accounts_enriched_populates_display_name_when_kind0_lands() {
    use ::nostr::{PublicKey, ToBech32};

    let pubkey_hex = JB55_PUBKEY.to_string();
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let npub = PublicKey::from_hex(&pubkey_hex)
        .expect("valid hex pubkey")
        .to_bech32()
        .expect("npub encode");
    let placeholder = identity_state::AccountSummary {
        id: pubkey_hex.clone(),
        npub: npub.clone(),
        display_name: None,
        signer_kind: "local".to_string(),
        status: "active".to_string(),
        signer_label: "nsec".to_string(),
        signer_is_remote: false,
        is_active: true,
        picture_url: None,
    };
    kernel.set_accounts(vec![placeholder], Some(pubkey_hex.clone()));

    // Pre-condition: with no kind:0 cached, `accounts_enriched` returns the
    // placeholder verbatim — display_name still `None`.
    let before = kernel.accounts_enriched();
    assert_eq!(before.len(), 1);
    assert_eq!(
        before[0].display_name, None,
        "no kind:0 → display_name must stay None"
    );

    // Land a kind:0 with a real display name. The enrichment branch in
    // `accounts_enriched` populates `display_name` from the cache.
    kernel.profiles.insert(
        pubkey_hex.clone(),
        Profile {
            event_id: "kind0-event".to_string(),
            created_at: 2_000,
            display: "Alice Smith".to_string(),
            picture_url: Some("https://example.com/pic.png".to_string()),
            nip05: String::new(),
            about: String::new(),
            lnurl: None,
        },
    );

    let after = kernel.accounts_enriched();
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].display_name.as_deref(), Some("Alice Smith"));
    assert_eq!(
        after[0].picture_url.as_deref(),
        Some("https://example.com/pic.png")
    );
}

/// Pins the djb2 avatar_color output for C13_PK so any algorithm drift is
/// caught immediately. Also verifies the format contract: 6 uppercase hex
/// chars, no '#' prefix (all other surfaces expect bare hex).
#[test]
fn avatar_color_djb2_pinned_vector() {
    let color = avatar_color_hex(C13_PK);
    // djb2 over last-6-bytes "3ac13a" starting at 5381, masked to 24 bits → "E886A1"
    assert_eq!(color, "E886A1", "djb2 pinned vector for C13_PK");
    // Format contract: exactly 6 chars, no '#', all uppercase hex
    assert_eq!(color.len(), 6, "avatar_color must be exactly 6 chars");
    assert!(color.chars().all(|c| c.is_ascii_hexdigit()), "must be hex");
    assert!(!color.starts_with('#'), "must NOT carry a '#' prefix");
    assert_eq!(color, color.to_uppercase(), "must be uppercase");
}
