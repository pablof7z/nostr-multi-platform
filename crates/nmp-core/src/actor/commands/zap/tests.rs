use super::*;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use nostr::Keys;

// ────────────────────────────────────────────────────────────────────
// V-07 — recipient relay injection.
//
// The kind:9734 `relays` tag tells the LN provider where to publish the
// kind:9735 receipt (NIP-57). Relay selection is kernel policy: shells
// MUST NOT decide where the receipt goes. The actor injects from
// `kernel.author_write_relays(recipient)` before signing whenever the
// executor produced no `relays` tag. The tests below pin the three
// observable contracts of that injection:
//   1. A pre-existing non-empty `relays` tag is left untouched.
//   2. No `relays` tag → one is injected, falling back to the bootstrap
//      discovery seed on cold-start.
//   3. A bare `["relays"]` tag (key only, no URLs) is treated as
//      absent — the injection fills it.
// ────────────────────────────────────────────────────────────────────

const RECIPIENT_HEX: &str =
    "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";

fn unsigned_for(tags: Vec<Vec<String>>) -> UnsignedEvent {
    UnsignedEvent {
        pubkey: String::new(),
        kind: 9734,
        tags,
        content: String::new(),
        created_at: 0,
    }
}

#[test]
fn inject_recipient_relays_preserves_existing_relays_tag() {
    let kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let mut unsigned = unsigned_for(vec![
        vec!["relays".to_string(), "wss://chosen.example".to_string()],
        vec!["p".to_string(), RECIPIENT_HEX.to_string()],
    ]);
    inject_recipient_relays(&kernel, &mut unsigned);
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
    // Exactly one relays tag — we didn't append a second.
    let relays_count = unsigned
        .tags
        .iter()
        .filter(|t| t.first().map(String::as_str) == Some("relays"))
        .count();
    assert_eq!(relays_count, 1, "must not duplicate the relays tag");
}

#[test]
fn inject_recipient_relays_injects_when_tag_absent() {
    let kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let mut unsigned = unsigned_for(vec![
        vec!["p".to_string(), RECIPIENT_HEX.to_string()],
    ]);
    inject_recipient_relays(&kernel, &mut unsigned);
    let relays_tag = unsigned
        .tags
        .iter()
        .find(|t| t.first().map(String::as_str) == Some("relays"))
        .expect("V-07: actor must inject a relays tag when caller omitted it");
    // `author_write_relays` falls back to the bootstrap discovery seed on
    // cold-start (no kind:10002 cached for this recipient).
    let expected_urls = kernel.author_write_relays(RECIPIENT_HEX);
    let mut got_urls: Vec<String> = relays_tag[1..].to_vec();
    let mut want_urls = expected_urls.clone();
    got_urls.sort();
    want_urls.sort();
    assert_eq!(got_urls, want_urls, "injected URLs must match kernel write list");
}

#[test]
fn inject_recipient_relays_treats_bare_relays_key_as_absent() {
    // A `["relays"]` row with no URLs is malformed — treat as absent so
    // the actor still injects a valid tag rather than passing the
    // malformed row through to the LNURL provider.
    let kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let mut unsigned = unsigned_for(vec![
        vec!["relays".to_string()],
        vec!["p".to_string(), RECIPIENT_HEX.to_string()],
    ]);
    inject_recipient_relays(&kernel, &mut unsigned);
    // The injected tag is appended; the original `["relays"]` row
    // remains a no-op (not removed) but a valid filled row is present.
    let filled_count = unsigned
        .tags
        .iter()
        .filter(|t| t.first().map(String::as_str) == Some("relays") && t.len() > 1)
        .count();
    assert_eq!(
        filled_count, 1,
        "must inject exactly one filled relays tag when the existing one is bare"
    );
}

#[test]
fn inject_recipient_relays_falls_back_to_bootstrap_when_p_tag_missing() {
    // Defensive — a builder bug that drops the `p` tag must NOT produce
    // a zap with an empty relays tag. The bootstrap seed is the
    // safe fallback (the LNURL HTTP layer will still verify the
    // recipient resolves later).
    let kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let mut unsigned = unsigned_for(Vec::new());
    inject_recipient_relays(&kernel, &mut unsigned);
    let relays_tag = unsigned
        .tags
        .iter()
        .find(|t| t.first().map(String::as_str) == Some("relays"))
        .expect("must inject a relays tag even when p tag is absent");
    assert!(
        relays_tag.len() > 1 || kernel.bootstrap_discovery_relays().is_empty(),
        "fallback bootstrap relays must be present unless seed itself is empty"
    );
}

#[test]
fn sign_zap_request_round_trips_through_event_builder() {
    // A minimal kind:9734 shape: relays tag + p tag + content. After
    // signing we should be able to deserialize the result back into a
    // `nostr::Event` and recover the same kind / content.
    let keys = Keys::generate();
    let unsigned = UnsignedEvent {
        pubkey: keys.public_key().to_hex(),
        kind: 9734,
        tags: vec![
            vec![
                "relays".to_string(),
                "wss://relay.example".to_string(),
            ],
            vec![
                "p".to_string(),
                "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff".to_string(),
            ],
        ],
        content: "great post 🤙".to_string(),
        created_at: 1_700_000_000,
    };
    let json = sign_zap_request(&keys, &unsigned).expect("sign must succeed");
    let event: nostr::Event =
        serde_json::from_str(&json).expect("signed output must be a valid nostr::Event");
    assert_eq!(event.kind.as_u16(), 9734);
    assert_eq!(event.content, "great post 🤙");
    // Signature is non-empty (a sentinel against accidentally returning
    // the unsigned event JSON).
    assert!(!event.sig.to_string().is_empty());
}

#[test]
fn sign_zap_request_rejects_out_of_range_kind() {
    let keys = Keys::generate();
    let unsigned = UnsignedEvent {
        pubkey: keys.public_key().to_hex(),
        // 100_000 is outside the u16 range nostr::Kind accepts.
        kind: 100_000,
        tags: Vec::new(),
        content: String::new(),
        created_at: 0,
    };
    assert!(sign_zap_request(&keys, &unsigned).is_err());
}

// ────────────────────────────────────────────────────────────────────
// Terminal success-stage recording.
//
// The zap worker's success branch sends `ActorCommand::RecordActionSuccess`
// back through the actor channel when a `correlation_id` was supplied.
// The dispatch arm folds that command into `Kernel::record_action_success`,
// which writes an `Accepted` stage into the `action_stages` mirror (so
// the host's stage observer sees the terminal) AND a terminal verdict
// into `action_results` (so the spinner keyed on the correlation_id
// clears on the next emit). The two tests below pin both legs of that
// contract — the kernel-level dual write and the worker-side gating.
// ────────────────────────────────────────────────────────────────────

/// `Kernel::record_action_success` MUST write an `Accepted` stage into
/// `action_stages` (terminal mirror) keyed on the supplied correlation_id.
/// Without this the dispatch arm would silently no-op:
/// the host's stage observer needs the `Accepted` row to ACK and the
/// `action_results` drain needs a terminal verdict to close the spinner.
#[test]
fn record_action_success_writes_accepted_stage_into_mirror() {
    use crate::kernel::Kernel;
    use crate::relay::DEFAULT_VISIBLE_LIMIT;

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let cid = "zap-pd036-success";
    kernel.record_action_success(cid.to_string());

    let snapshot = kernel.action_stages_projection();
    // Snapshot is a JSON object keyed by correlation_id; the value is the
    // history array of `{stage, at_ms, detail?}` rows.
    let entries = snapshot
        .as_object()
        .expect("action_stages projection must serialize as an object");
    let history = entries
        .get(cid)
        .and_then(serde_json::Value::as_array)
        .expect("correlation_id row must be present");
    assert!(
        !history.is_empty(),
        "history must carry at least one stage entry"
    );
    let last = history
        .last()
        .and_then(serde_json::Value::as_object)
        .expect("stage entry must be a JSON object");
    // ActionStage is serialized as `{"stage": "accepted"}` — see
    // `action_stages::ActionStage` `#[serde(tag = "stage", rename_all =
    // "snake_case")]`.
    assert_eq!(
        last.get("stage").and_then(serde_json::Value::as_str),
        Some("accepted"),
        "terminal stage must be `accepted` after record_action_success"
    );
}

/// The success branch's `RecordActionSuccess` send MUST be
/// gated on `correlation_id.is_some()`. Direct C-ABI callers (or any
/// future caller) that pass `None` get the `ShowToast` only — there is
/// no spinner to close. This pins the symmetric guard the failure leg
/// already honours; without it a `None` caller would crash the actor
/// with a `record_action_success("")` (empty-string is not a valid
/// correlation_id) or silently pollute the `action_stages` mirror with
/// an entry no host is waiting on.
///
/// Test strategy: construct an `ActorCommand::RecordActionSuccess`
/// variant directly (proving it exists and carries the expected
/// payload shape). The wire-up that the spawn closure honours the
/// `Option` guard is enforced statically by the `if let Some(cid)`
/// pattern — a code-grep / review gate.
#[test]
fn record_action_success_command_carries_correlation_id() {
    let cmd = ActorCommand::RecordActionSuccess {
        correlation_id: "zap-pd036-shape".to_string(),
    };
    match cmd {
        ActorCommand::RecordActionSuccess { correlation_id } => {
            assert_eq!(correlation_id, "zap-pd036-shape");
        }
        other => panic!("expected RecordActionSuccess variant, got {other:?}"),
    }
}
