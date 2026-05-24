use super::*;
use std::sync::Arc;

/// The group every test event in this module targets.
fn group() -> GroupId {
    GroupId::new("wss://groups.example.com", "rust-nostr")
}

/// Build a `KernelEvent` with an explicit kind and tag set.
fn event(id: &str, kind: u32, created_at: u64, tags: Vec<Vec<String>>) -> KernelEvent {
    KernelEvent {
        id: id.into(),
        author: format!("author-of-{id}"),
        kind,
        created_at,
        tags,
        content: format!("content of {id}"),
    }
}

/// `["h", local_id]` for the test group.
fn h_tag(local_id: &str) -> Vec<Vec<String>> {
    vec![vec!["h".into(), local_id.into()]]
}

#[test]
fn fresh_projection_yields_empty_messages_with_group_initials() {
    // V-29: a fresh projection has no messages but DOES carry the
    // group-initials label derived from `local_id` ("rust-nostr" → "RU").
    // `GroupChatSnapshot::empty()` is the D6 fallback for a poisoned lock
    // (no `local_id` reachable there) and reports `"?"` instead — the
    // two paths now diverge on `group_initials` by design, so we assert
    // structure rather than full equality.
    let proj = GroupChatProjection::new(group());
    let snap = proj.snapshot();
    assert!(snap.messages.is_empty());
    assert_eq!(snap.group_initials, "RU");
    let json = proj.snapshot_json();
    assert_eq!(
        json,
        serde_json::json!({ "messages": [], "group_initials": "RU" })
    );
}

#[test]
fn matching_chat_message_is_retained() {
    let proj = GroupChatProjection::new(group());
    proj.on_kernel_event(&event("e1", KIND_CHAT_MESSAGE, 100, h_tag("rust-nostr")));

    let snap = proj.snapshot();
    assert_eq!(snap.messages.len(), 1);
    let msg = &snap.messages[0];
    assert_eq!(msg.id, "e1");
    assert_eq!(msg.pubkey, "author-of-e1");
    assert_eq!(msg.content, "content of e1");
    assert_eq!(msg.created_at, 100);
    assert_eq!(msg.kind, KIND_CHAT_MESSAGE);
}

#[test]
fn thread_kind_is_retained() {
    let proj = GroupChatProjection::new(group());
    proj.on_kernel_event(&event(
        "thread",
        KIND_DISCUSSION_OR_ARTIFACT,
        10,
        h_tag("rust-nostr"),
    ));

    let snap = proj.snapshot();
    assert_eq!(snap.messages.len(), 1);
    let kinds: Vec<u32> = snap.messages.iter().map(|m| m.kind).collect();
    assert!(kinds.contains(&KIND_DISCUSSION_OR_ARTIFACT));
}

#[test]
fn event_for_a_different_group_is_excluded() {
    let proj = GroupChatProjection::new(group());
    // Correct kind, but the `h` tag names a different group.
    proj.on_kernel_event(&event("other", KIND_CHAT_MESSAGE, 100, h_tag("some-other-room")));
    assert!(proj.snapshot().messages.is_empty());
}

#[test]
fn event_with_no_h_tag_is_excluded() {
    let proj = GroupChatProjection::new(group());
    // Correct kind, but no `h` tag at all — not a group event.
    proj.on_kernel_event(&event("loose", KIND_CHAT_MESSAGE, 100, vec![]));
    assert!(proj.snapshot().messages.is_empty());
}

#[test]
fn off_kind_event_with_matching_h_tag_is_excluded() {
    let proj = GroupChatProjection::new(group());
    // kind 1 (plain note) and kind 9000 (a moderation action) both carry a
    // matching `h` tag, but neither is a chat-content kind.
    proj.on_kernel_event(&event("note", 1, 100, h_tag("rust-nostr")));
    proj.on_kernel_event(&event("modaction", 9000, 100, h_tag("rust-nostr")));
    assert!(proj.snapshot().messages.is_empty());
}

#[test]
fn messages_are_ordered_newest_first() {
    let proj = GroupChatProjection::new(group());
    // Deliver out of chronological order.
    proj.on_kernel_event(&event("mid", KIND_CHAT_MESSAGE, 200, h_tag("rust-nostr")));
    proj.on_kernel_event(&event("old", KIND_CHAT_MESSAGE, 100, h_tag("rust-nostr")));
    proj.on_kernel_event(&event("new", KIND_CHAT_MESSAGE, 300, h_tag("rust-nostr")));

    let snap = proj.snapshot();
    let ids: Vec<&str> = snap.messages.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, vec!["new", "mid", "old"]);
}

#[test]
fn created_at_ties_break_on_id_descending() {
    let proj = GroupChatProjection::new(group());
    // Same `created_at` — order must still be total and deterministic.
    proj.on_kernel_event(&event("aaa", KIND_CHAT_MESSAGE, 500, h_tag("rust-nostr")));
    proj.on_kernel_event(&event("zzz", KIND_CHAT_MESSAGE, 500, h_tag("rust-nostr")));

    let snap = proj.snapshot();
    let ids: Vec<&str> = snap.messages.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, vec!["zzz", "aaa"]);
}

#[test]
fn duplicate_event_id_is_not_duplicated() {
    let proj = GroupChatProjection::new(group());
    let evt = event("dup", KIND_CHAT_MESSAGE, 100, h_tag("rust-nostr"));
    proj.on_kernel_event(&evt);
    proj.on_kernel_event(&evt);

    let snap = proj.snapshot();
    assert_eq!(snap.messages.len(), 1, "re-delivered id must not duplicate");
}

#[test]
fn snapshot_json_contains_the_messages() {
    let proj = GroupChatProjection::new(group());
    proj.on_kernel_event(&event("e1", KIND_CHAT_MESSAGE, 100, h_tag("rust-nostr")));
    proj.on_kernel_event(&event(
        "e2",
        KIND_DISCUSSION_OR_ARTIFACT,
        200,
        h_tag("rust-nostr"),
    ));

    let json = proj.snapshot_json();
    let messages = json
        .get("messages")
        .and_then(|m| m.as_array())
        .expect("snapshot json has a `messages` array");
    assert_eq!(messages.len(), 2);
    // Newest-first: e2 (created_at 200) precedes e1 (created_at 100).
    assert_eq!(messages[0].get("id").and_then(|v| v.as_str()), Some("e2"));
    assert_eq!(messages[1].get("id").and_then(|v| v.as_str()), Some("e1"));
    // Field shape: `pubkey` carries `KernelEvent::author`.
    assert_eq!(
        messages[0].get("pubkey").and_then(|v| v.as_str()),
        Some("author-of-e2"),
    );
}

#[test]
fn round_trips_through_serde() {
    let proj = GroupChatProjection::new(group());
    proj.on_kernel_event(&event("e1", KIND_CHAT_MESSAGE, 100, h_tag("rust-nostr")));
    let snap = proj.snapshot();
    let encoded = serde_json::to_string(&snap).expect("snapshot serialises");
    let decoded: GroupChatSnapshot =
        serde_json::from_str(&encoded).expect("snapshot deserialises");
    assert_eq!(snap, decoded);
}

#[test]
fn drives_through_observer_trait_object() {
    // The projection must be usable as `Arc<dyn KernelEventObserver>` —
    // that is exactly how a host registers it with `register_event_observer`.
    let proj = Arc::new(GroupChatProjection::new(group()));
    let observer: Arc<dyn KernelEventObserver> = Arc::clone(&proj) as _;
    observer.on_kernel_event(&event("e1", KIND_CHAT_MESSAGE, 100, h_tag("rust-nostr")));
    assert_eq!(proj.snapshot().messages.len(), 1);
}

#[test]
fn group_id_accessor_returns_construction_value() {
    let proj = GroupChatProjection::new(group());
    assert_eq!(proj.group_id(), &group());
}

// ── V-22: created_at_display relative-time labels ────────────────────
//
// The exhaustive bucket tests live in `nmp_core::display::tests`. The
// projection-level tests below verify the snapshot path threads the
// pinned wall-clock through `nmp_core::display::format_ago_secs` correctly
// — that is the behaviour the host shell binds to.

#[test]
fn snapshot_at_populates_created_at_display() {
    // Pin the clock so the bucket assertions are deterministic.
    let proj = GroupChatProjection::new(group());
    // Three events at known offsets from `now = 10_000`.
    proj.on_kernel_event(&event("seconds", KIND_CHAT_MESSAGE, 10_000 - 5, h_tag("rust-nostr")));
    proj.on_kernel_event(&event("minutes", KIND_CHAT_MESSAGE, 10_000 - 120, h_tag("rust-nostr")));
    proj.on_kernel_event(&event("hours", KIND_CHAT_MESSAGE, 10_000 - 7_200, h_tag("rust-nostr")));

    let snap = proj.snapshot_at(10_000);
    // Newest-first: seconds (now-5), minutes (now-120), hours (now-7200).
    assert_eq!(snap.messages[0].id, "seconds");
    assert_eq!(snap.messages[0].created_at_display, "5s ago");
    assert_eq!(snap.messages[1].id, "minutes");
    assert_eq!(snap.messages[1].created_at_display, "2m ago");
    assert_eq!(snap.messages[2].id, "hours");
    assert_eq!(snap.messages[2].created_at_display, "2h ago");
}

#[test]
fn snapshot_at_refreshes_created_at_display_across_ticks() {
    // The label must be (re)computed every tick — never stale.
    let proj = GroupChatProjection::new(group());
    proj.on_kernel_event(&event("e1", KIND_CHAT_MESSAGE, 1_000, h_tag("rust-nostr")));

    // Tick at `now = 1_005` → "5s ago".
    let snap_a = proj.snapshot_at(1_005);
    assert_eq!(snap_a.messages[0].created_at_display, "5s ago");

    // Tick at `now = 1_065` → "1m ago" (same underlying message).
    let snap_b = proj.snapshot_at(1_065);
    assert_eq!(snap_b.messages[0].created_at_display, "1m ago");
}

#[test]
fn ingest_leaves_created_at_display_empty() {
    // The ingest path must NOT pre-compute the label — only the snapshot
    // path may, so the label cannot leak a stale value if a code path
    // ever reads the bounded store directly.
    let proj = GroupChatProjection::new(group());
    proj.on_kernel_event(&event("e1", KIND_CHAT_MESSAGE, 100, h_tag("rust-nostr")));

    // Drop into the internal store directly to assert the ingest-time
    // shape. (Tests live in-module so this private field is reachable.)
    let messages = proj.messages.lock().expect("lock");
    let stored = messages
        .values()
        .next()
        .expect("one message stored");
    assert_eq!(stored.created_at_display, "", "ingest must not populate the label");
}

#[test]
fn snapshot_json_includes_created_at_display() {
    let proj = GroupChatProjection::new(group());
    proj.on_kernel_event(&event("e1", KIND_CHAT_MESSAGE, 100, h_tag("rust-nostr")));

    let json = proj.snapshot_json();
    let msg = &json["messages"][0];
    // Even with the production `snapshot()` clock the field is present
    // and non-empty — for any sane host clock it renders as `Xd ago`.
    let display = msg["created_at_display"]
        .as_str()
        .expect("created_at_display present");
    assert!(!display.is_empty(), "snapshot_json must populate the label");
}

// ── V-25: author display strings ──────────────────────────────────────
//
// Pubkey-derived display strings live in Rust now — the iOS view binds
// the three fields directly and Swift-side helpers are deleted.
// `author_display` uses `short_npub` (V-33); the canonical algorithm
// coverage lives in `nmp_core::display::tests`.

// The pinned cross-surface djb2 vector lives in `nmp_core::display::tests`
// (`avatar_color_hex_matches_pinned_djb2_vector`); the
// `ingest_populates_author_display_strings` / `snapshot_preserves_author_display_strings`
// tests below anchor the call-site value (`"author-of-e1" → "E8844A"`)
// so a drift in the canonical helper is still caught at this layer.

#[test]
fn ingest_populates_author_display_strings() {
    // The ingest path SHOULD pre-compute the author display strings —
    // unlike `created_at_display`, these are pure functions of the
    // pubkey hex and do not change across snapshot ticks, so recomputing
    // them per tick would be wasted work. Reach into the internal store
    // to assert the ingest-time shape.
    let proj = GroupChatProjection::new(group());
    proj.on_kernel_event(&event("e1", KIND_CHAT_MESSAGE, 100, h_tag("rust-nostr")));

    let messages = proj.messages.lock().expect("lock");
    let stored = messages.values().next().expect("one message stored");
    // `event("e1", …)` synthesises author = "author-of-e1" (len 12 < 16),
    // so `author_display` is the raw author and `author_initials` is the
    // first two chars uppercased.
    assert_eq!(stored.pubkey, "author-of-e1");
    assert_eq!(stored.author_display, "author-of-e1");
    assert_eq!(stored.author_initials, "AU");
    assert_eq!(stored.author_color_hex, "E8844A");
}

#[test]
fn snapshot_preserves_author_display_strings() {
    // The snapshot path must surface the ingest-time author fields
    // unchanged — only `created_at_display` is rewritten per tick.
    let proj = GroupChatProjection::new(group());
    proj.on_kernel_event(&event("e1", KIND_CHAT_MESSAGE, 100, h_tag("rust-nostr")));

    let snap = proj.snapshot_at(200);
    let msg = &snap.messages[0];
    assert_eq!(msg.author_display, "author-of-e1");
    assert_eq!(msg.author_initials, "AU");
    assert_eq!(msg.author_color_hex, "E8844A");
}

#[test]
fn snapshot_json_includes_author_display_strings() {
    // The Swift `GroupChatMessage` decoder uses `.convertFromSnakeCase`,
    // so the snake_case keys here are exactly what the host binds to
    // `authorDisplay` / `authorInitials` / `authorColorHex`.
    let proj = GroupChatProjection::new(group());
    proj.on_kernel_event(&event("e1", KIND_CHAT_MESSAGE, 100, h_tag("rust-nostr")));

    let json = proj.snapshot_json();
    let msg = &json["messages"][0];
    assert_eq!(
        msg["author_display"].as_str(),
        Some("author-of-e1"),
        "snapshot_json must emit author_display"
    );
    assert_eq!(
        msg["author_initials"].as_str(),
        Some("AU"),
        "snapshot_json must emit author_initials"
    );
    assert_eq!(
        msg["author_color_hex"].as_str(),
        Some("E8844A"),
        "snapshot_json must emit author_color_hex"
    );
}

#[test]
fn snapshot_with_realistic_pubkey_abbreviates_correctly() {
    // Drive `from_event` with a real 64-char hex author so the
    // production-shape `author_display` branch is exercised end to end.
    let realistic_author =
        "abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789";
    let evt = KernelEvent {
        id: "e-real".into(),
        author: realistic_author.into(),
        kind: KIND_CHAT_MESSAGE,
        created_at: 100,
        tags: h_tag("rust-nostr"),
        content: "hello".into(),
    };
    let proj = GroupChatProjection::new(group());
    proj.on_kernel_event(&evt);

    let snap = proj.snapshot_at(200);
    let msg = &snap.messages[0];
    // short_npub: first 10 chars of npub1… + "…" + last 6 chars (V-33).
    assert_eq!(msg.author_display, "npub140x77…tddknj");
    // avatar_initials(to_npub(hex)) — first 2 chars of the bech32 body.
    // The pinned value "40" comes from the canonical npub encoding of
    // the test fixture; any drift here means the bech32 helper changed.
    assert_eq!(msg.author_initials, "40");
    assert_eq!(msg.author_color_hex, "08E60C");
}

// ── V-29: group_initials avatar-tile label ────────────────────────────
//
// The iOS `PublicGroupRow` used to derive `initials` from `groupId.localId`
// in Swift. That derivation lives in Rust now; the view binds the snapshot
// field directly. These tests pin the algorithm — a drift here changes
// what every NIP-29 group row renders for its avatar tile.

#[test]
fn group_initials_takes_first_two_uppercased() {
    assert_eq!(group_initials("rust-nostr"), "RU");
    assert_eq!(group_initials("ab"), "AB");
    // Mixed case input is normalised to uppercase.
    assert_eq!(group_initials("aBcDeF"), "AB");
}

#[test]
fn group_initials_empty_input_is_question_mark() {
    // Matches the deleted Swift `guard !id.isEmpty else { return "?" }`
    // fallback — the host always has a non-empty label to render.
    assert_eq!(group_initials(""), "?");
}

#[test]
fn group_initials_single_char_input_is_uppercased() {
    // D6: no panic on shorter-than-two inputs; the available prefix is
    // returned uppercased.
    assert_eq!(group_initials("a"), "A");
}

#[test]
fn snapshot_populates_group_initials_from_local_id() {
    // The fixture group has `local_id = "rust-nostr"` → "RU".
    let proj = GroupChatProjection::new(group());
    assert_eq!(proj.snapshot().group_initials, "RU");
}

#[test]
fn snapshot_group_initials_is_refreshed_per_tick_against_local_id() {
    // The group_initials field must reflect the projection's own group
    // identity on every snapshot — not the empty-fallback `"?"`. Even
    // after ingesting messages, the label stays bound to `local_id`.
    let proj = GroupChatProjection::new(group());
    proj.on_kernel_event(&event("e1", KIND_CHAT_MESSAGE, 100, h_tag("rust-nostr")));

    let snap = proj.snapshot_at(200);
    assert_eq!(snap.group_initials, "RU");
    assert_eq!(snap.messages.len(), 1);
}

#[test]
fn snapshot_json_includes_group_initials() {
    // The Swift `GroupChatSnapshot` decoder uses `.convertFromSnakeCase`,
    // so the snake_case key here is exactly what the host binds to
    // `groupInitials`. This is the load-bearing assertion that V-29 lets
    // the iOS `PublicGroupRow` delete its Swift `initials` derivation.
    let proj = GroupChatProjection::new(group());
    let json = proj.snapshot_json();
    assert_eq!(
        json["group_initials"].as_str(),
        Some("RU"),
        "snapshot_json must emit group_initials for the public-group avatar tile"
    );
}

#[test]
fn empty_snapshot_constructor_uses_question_mark_label() {
    // `GroupChatSnapshot::empty()` runs on the poisoned-lock D6 path where
    // no `local_id` is reachable — it must still have a non-empty
    // `group_initials` so the host avatar tile is never blank.
    let empty = GroupChatSnapshot::empty();
    assert!(empty.messages.is_empty());
    assert_eq!(empty.group_initials, "?");
}

#[test]
fn snapshot_serde_round_trips_group_initials() {
    // The `group_initials` field survives a serde round trip end to end
    // (the iOS shell decodes the same JSON shape).
    let proj = GroupChatProjection::new(group());
    proj.on_kernel_event(&event("e1", KIND_CHAT_MESSAGE, 100, h_tag("rust-nostr")));
    let snap = proj.snapshot();
    let encoded = serde_json::to_string(&snap).expect("snapshot serialises");
    let decoded: GroupChatSnapshot =
        serde_json::from_str(&encoded).expect("snapshot deserialises");
    assert_eq!(decoded.group_initials, "RU");
    assert_eq!(decoded.messages.len(), 1);
}
