//! T125 delivering-relay URL tagging on the kind:22242 AUTH event, and
//! per-role `auth_signers` isolation (a signer bound to one `RelayRole` must
//! not answer another role's challenge).

use super::*;

// ───────────────────────────────────────────────────────────────────────────
// T125 — kind:22242 `relay` tag and outbound routing key must reference the
//        DELIVERING relay's URL, not the role's bootstrap URL.
// ───────────────────────────────────────────────────────────────────────────
//
// NIP-42 binds the AUTH event to the URL of the relay that issued the
// challenge: the `["relay", <url>]` tag is the canonical anti-replay
// surface, and relays validate it against the socket the AUTH arrived on.
// Pre-T125 the kernel stamped `role.url()` (i.e. the lane's bootstrap host
// — the shared content bootstrap URL for the Content lane) regardless of which
// resolved relay sent the CHALLENGE. After T105's URL-keyed transport pool
// (`fada22b`), that ALSO routed the AUTH response to the wrong socket.
//
// Two distinct relays issue distinct challenges; each AUTH response must
// carry the matching delivering URL on BOTH the `relay` tag AND the
// outbound `relay_url` routing field. We parse the wire frames rather than
// substring-match, because the bootstrap URL and
// our test URLs ("wss://a.example", "wss://b.example") would otherwise
// require fragile contains/disjointness checks.
#[test]
fn nip42_kind_22242_tags_delivering_relay_url_not_bootstrap() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // Two signer instances with distinct fixed event ids so each AUTH
    // dispatch has an independently correlatable id (the advisor's note —
    // a single signer would have the second `record_dispatch` overwrite
    // the first's pending_event_id, but the bug under test is the OUTBOUND
    // frame contents, not the driver state, so we never drive OK here).
    let url_a = "wss://a.example";
    let url_b = "wss://b.example";

    // First relay's challenge → first signed AUTH.
    let (signer_a, _) = make_signer(AUTH_EVENT_ID);
    kernel.bind_auth_signer(SIGNER_PUBKEY.to_string(), signer_a);
    let outbound_a = kernel.handle_text(RelayRole::Content, url_a, &auth_frame("ch-a"));

    let auth_frame_a = outbound_a
        .iter()
        .find(|m| m.text.starts_with("[\"AUTH\""))
        .expect("AUTH outbound from relay A");
    assert_eq!(
        auth_frame_a.relay_url, url_a,
        "T125: OutboundMessage.relay_url must equal the delivering URL so \
         the URL-keyed transport (fada22b) dials the right socket; \
         pre-T125 this stamped role.url() = bootstrap"
    );
    let relay_tag_a = extract_relay_tag(&auth_frame_a.text);
    assert_eq!(
        relay_tag_a,
        url_a,
        "T125: kind:22242 `relay` tag must equal the delivering URL per \
         NIP-42 (anti-replay binds AUTH to the issuing relay); pre-T125 \
         this stamped role.url() = bootstrap ({:?})",
        RelayRole::Content.url()
    );

    // Second relay's challenge — rebind a signer with a distinct event id
    // so the driver's pending_event_id correlation is unambiguous, then
    // confirm the second AUTH carries url_b on both fields.
    let (signer_b, _) = make_signer(AUTH_EVENT_ID_2);
    kernel.bind_auth_signer(SIGNER_PUBKEY.to_string(), signer_b);
    let outbound_b = kernel.handle_text(RelayRole::Content, url_b, &auth_frame("ch-b"));

    let auth_frame_b = outbound_b
        .iter()
        .find(|m| m.text.starts_with("[\"AUTH\""))
        .expect("AUTH outbound from relay B");
    assert_eq!(
        auth_frame_b.relay_url, url_b,
        "T125: second AUTH must route to delivering URL B"
    );
    let relay_tag_b = extract_relay_tag(&auth_frame_b.text);
    assert_eq!(
        relay_tag_b, url_b,
        "T125: second AUTH `relay` tag must equal delivering URL B"
    );

    // Crucial cross-check: the two AUTH frames carry DISTINCT relay tags.
    // Pre-T125 they'd both be `RelayRole::Content.url()` (bootstrap) and
    // this would fail with a clear A=B mismatch.
    assert_ne!(
        relay_tag_a, relay_tag_b,
        "T125: distinct delivering URLs must produce distinct AUTH `relay` tags"
    );
}

/// Parse `["AUTH", {<event>}]` text and return the value of the `relay` tag.
/// Panics with a helpful message if the frame shape is unexpected — these
/// are tests, so loud failure is the right policy.
fn extract_relay_tag(wire: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(wire).expect("AUTH frame is valid JSON");
    let arr = v.as_array().expect("AUTH frame is a JSON array");
    assert_eq!(arr.first().and_then(|s| s.as_str()), Some("AUTH"));
    let event = arr.get(1).expect("AUTH frame has event payload");
    let tags = event
        .get("tags")
        .and_then(|t| t.as_array())
        .expect("event has tags array");
    for tag in tags {
        let parts = tag.as_array().expect("tag is array");
        if parts.first().and_then(|s| s.as_str()) == Some("relay") {
            return parts
                .get(1)
                .and_then(|s| s.as_str())
                .expect("relay tag has url")
                .to_string();
        }
    }
    panic!("no `relay` tag found in AUTH event: {wire}")
}

// ───────────────────────────────────────────────────────────────────────────
// Per-role signer isolation — `set_relay_auth_signer` binds ONE role.
//
// The handshake tests above use `bind_auth_signer`, which binds Content AND
// Indexer with one call, so the per-role `auth_signers` map's isolation is
// never exercised. These tests pin the `RelayRole`-keyed lookup in
// `handle_auth_challenge`: a signer bound to one role must not answer a
// challenge delivered on another. A regression here would silently sign an
// AUTH event for the wrong lane's relay (D0: AUTH state is per-transport).
// ───────────────────────────────────────────────────────────────────────────

/// A signer bound to `Content` only does NOT answer an `Indexer` challenge:
/// the Indexer driver stays in `ChallengeReceived` and no AUTH frame is sent.
#[test]
fn nip42_per_role_signer_does_not_answer_other_role_challenge() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let (signer, calls) = make_signer(AUTH_EVENT_ID);
    // Bind ONLY the Content role (per-role primitive, not the compat wrapper).
    kernel.set_relay_auth_signer(RelayRole::Content, SIGNER_PUBKEY.to_string(), signer);

    // Challenge arrives on the Indexer lane, which has no signer.
    let outbound = kernel.handle_text(
        RelayRole::Indexer,
        RelayRole::Indexer.url(),
        &auth_frame("ch-idx"),
    );

    assert_eq!(
        *calls.lock().unwrap(),
        0,
        "a Content-bound signer must not be invoked for an Indexer challenge",
    );
    assert!(
        outbound.is_empty(),
        "no signer for the Indexer role => no AUTH wire frame emitted",
    );
    assert_eq!(
        auth_state_of(&kernel, RelayRole::Indexer),
        RelayAuthState::ChallengeReceived,
        "the Indexer driver records the challenge but stays unanswered",
    );
}

/// With a signer bound to `Content` only, a `Content` challenge IS answered
/// while a concurrent `Indexer` challenge is NOT — the two lanes resolve
/// independently against the per-role `auth_signers` map.
#[test]
fn nip42_signer_answers_bound_role_only_across_concurrent_challenges() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let (signer, calls) = make_signer(AUTH_EVENT_ID);
    kernel.set_relay_auth_signer(RelayRole::Content, SIGNER_PUBKEY.to_string(), signer);

    // Indexer challenge first — unanswered (no signer for that role).
    let idx_out = kernel.handle_text(
        RelayRole::Indexer,
        RelayRole::Indexer.url(),
        &auth_frame("ch-idx"),
    );
    assert!(idx_out.is_empty(), "Indexer has no signer => no AUTH frame");

    // Content challenge — answered by the bound signer.
    let content_out = kernel.handle_text(
        RelayRole::Content,
        RelayRole::Content.url(),
        &auth_frame("ch-content"),
    );
    assert_eq!(
        *calls.lock().unwrap(),
        1,
        "the Content-bound signer fires exactly once, for the Content challenge",
    );
    assert!(
        content_out
            .iter()
            .any(|m| m.role == RelayRole::Content && m.text.starts_with("[\"AUTH\"")),
        "the Content challenge is answered with a kind:22242 AUTH frame",
    );

    assert_eq!(
        auth_state_of(&kernel, RelayRole::Content),
        RelayAuthState::Authenticating,
        "the Content lane advances to Authenticating",
    );
    assert_eq!(
        auth_state_of(&kernel, RelayRole::Indexer),
        RelayAuthState::ChallengeReceived,
        "the Indexer lane stays in ChallengeReceived — its signer is unbound",
    );
}

/// `handle_auth_challenge` stores the verbatim challenge string on the
/// per-role NIP-42 driver. The handshake tests assert state transitions but
/// never the stored challenge value directly; this pins that the kernel
/// retains the exact challenge it must echo back in the kind:22242 tag.
#[test]
fn nip42_auth_challenge_is_stored_verbatim_on_driver() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // No signer bound — isolates the challenge-capture step from signing.
    let _ = kernel.handle_text(
        RelayRole::Content,
        RelayRole::Content.url(),
        &auth_frame("challenge-xyz-123"),
    );

    let driver = kernel
        .auth_drivers
        .get(&RelayRole::Content)
        .expect("an AUTH challenge must create the per-role driver entry");
    assert_eq!(
        driver.pending_challenge.as_deref(),
        Some("challenge-xyz-123"),
        "the kernel stores the relay's challenge string verbatim",
    );
    assert_eq!(
        driver.state,
        RelayAuthState::ChallengeReceived,
        "the driver transitions to ChallengeReceived on the AUTH frame",
    );
}
