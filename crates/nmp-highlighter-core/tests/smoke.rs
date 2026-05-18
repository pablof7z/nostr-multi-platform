//! Smoke tests for `nmp-highlighter-core` public surface.
//!
//! ## Scope (M11.5 Step 0)
//!
//! The crate ships as a **scaffold** — the full `ReadsFeed`, `SearchIndex`,
//! `CaptureFlow`, `Feedback`, `WebMetadata`, `IsbnLookup`, `BookRegistry`, and
//! `PublishHighlightAndShareToGroup` impls land in Steps 3 + 5.  These tests
//! verify the types that *are* present on master and will be extended when the
//! richer surfaces ship.
//!
//! Types tested:
//! - `Step0Scaffold` — construction, `Clone`, `Debug`, `Default`, `PartialEq`,
//!   and serde round-trip.
//! - `GroupId` re-export — confirm the re-export is accessible without
//!   importing `nmp-nip29` directly, proving the dependency graph edge is
//!   correct.
//!
//! Follow-up: extend this file with `Highlight`, `Article`, `Note`, `Reaction`
//! smoke tests once those types land (M11.5 Steps 3 + 5).

use nmp_highlighter_core::placeholders::Step0Scaffold;
use nmp_highlighter_core::GroupId;

// ── Step0Scaffold ─────────────────────────────────────────────────────────────

#[test]
fn step0_scaffold_default_construction() {
    let s = Step0Scaffold;
    assert_eq!(s, Step0Scaffold);
}

#[test]
fn step0_scaffold_copy_eq() {
    let a = Step0Scaffold;
    // Step0Scaffold implements Copy; verify the value is usable after being
    // passed by copy (no explicit clone needed).
    let b = a;
    assert_eq!(a, b);
}

#[test]
fn step0_scaffold_debug_non_empty() {
    let s = Step0Scaffold;
    let repr = format!("{s:?}");
    assert!(!repr.is_empty(), "Debug repr must not be empty");
}

#[test]
fn step0_scaffold_serde_round_trip() {
    let original = Step0Scaffold;
    let json = serde_json::to_string(&original).expect("serialise");
    let restored: Step0Scaffold = serde_json::from_str(&json).expect("deserialise");
    assert_eq!(original, restored);
}

#[test]
fn step0_scaffold_serde_is_stable_null() {
    // Step0Scaffold is a unit struct — its canonical JSON form is `null`.
    let json = serde_json::to_string(&Step0Scaffold).expect("serialise");
    assert_eq!(json, "null", "unit struct serialises to null");
    let restored: Step0Scaffold = serde_json::from_str("null").expect("deserialise null");
    assert_eq!(restored, Step0Scaffold);
}

// ── GroupId re-export ─────────────────────────────────────────────────────────

#[test]
fn group_id_reexport_accessible() {
    // Confirm that `nmp_highlighter_core::GroupId` resolves without importing
    // `nmp_nip29` directly — validates the dependency graph edge.
    let g = GroupId::new("wss://groups.example.com", "room-a");
    assert_eq!(g.host_relay_url, "wss://groups.example.com");
    assert_eq!(g.local_id, "room-a");
}

#[test]
fn group_id_reexport_uri_codec() {
    let g = GroupId::new("wss://groups.example.com", "room-a");
    let uri = g.to_uri();
    let parsed = GroupId::from_uri(&uri).expect("round-trip uri");
    assert_eq!(parsed, g, "URI round-trip must preserve GroupId fields");
}

#[test]
fn group_id_reexport_serde_round_trip() {
    let g = GroupId::new("wss://relay.example.com", "my-group");
    let json = serde_json::to_string(&g).expect("serialise");
    let restored: GroupId = serde_json::from_str(&json).expect("deserialise");
    assert_eq!(g, restored);
}

#[test]
fn group_id_reexport_clone_eq_ord() {
    let g1 = GroupId::new("wss://relay.example.com", "alpha");
    let g2 = GroupId::new("wss://relay.example.com", "beta");
    let g1b = g1.clone();
    assert_eq!(g1, g1b);
    assert!(g1 < g2, "GroupId Ord must work (lexicographic on local_id)");
}
