//! Locked-invariant tests for the `SnapshotProjections` dotted-projection-key
//! registry. Extracted from `swift_projections_registry.rs` as a sibling test
//! module to keep the registry source under the file-size ceiling; the parent
//! re-attaches it via `#[path]` so `super::*` continues to resolve against the
//! registry definitions.

use super::*;

/// Locks the registry size. Anyone adding or removing an entry changes
/// the generated Swift; this test makes that change explicit rather than
/// silent.
#[test]
fn registry_size_is_locked() {
    // 27 entries: 31 (the #1610 baseline) minus 3 old-surface entries removed
    // in ADR-0063 Lane H (#1671): `claimed_profiles` (KCPR),
    // `resolved_profiles` (KRPR), and the host-visible `claimed_events`
    // whole-map projection. KCEV remains only as the refs.event row codec.
    // Profile/event data is now served via refs.profile / refs.event NRRD
    // row-delta sidecars; minus the deleted global zaps sidecar (#2091).
    // Bump this (and add a new SnapshotProjectionEntry above) when a new
    // projection is wired.
    assert_eq!(
        SNAPSHOT_PROJECTIONS.len(),
        27,
        "registry size changed — regenerate KernelTypes.generated.swift and update this test"
    );
}

/// Every Swift field name must be a unique lowerCamelCase identifier.
/// A duplicate would emit two `let` lines with the same name (Swift
/// compile error in the generated file) — this guards against an
/// accidental copy/paste regression.
#[test]
fn swift_field_names_are_unique() {
    let mut seen = std::collections::BTreeSet::new();
    for entry in SNAPSHOT_PROJECTIONS {
        assert!(
            seen.insert(entry.swift_field),
            "duplicate swift_field {:?} in SNAPSHOT_PROJECTIONS",
            entry.swift_field
        );
    }
}

/// Every projection key must be unique. The kernel registers one closure per
/// key; declaring the same key twice on the Swift side would silently
/// shadow one decoder case with another.
#[test]
fn projection_keys_are_unique() {
    let mut seen = std::collections::BTreeSet::new();
    for entry in SNAPSHOT_PROJECTIONS {
        assert!(
            seen.insert(entry.key),
            "duplicate key {:?} in SNAPSHOT_PROJECTIONS",
            entry.key
        );
    }
}

/// Every dotted projection key in this registry must be mirrored in the
/// conformance test (`SnapshotProjectionsConformanceTests.swift`) and vice
/// versa — except `nmp.feed.home`, the typed sidecar (see below). Adding a
/// dotted key requires updating both sides (and the renderer emits a
/// matching `CodingKeys` case for JSON-decoded keys).
#[test]
fn all_dotted_keys_are_present() {
    let dotted: Vec<&str> = SNAPSHOT_PROJECTIONS
        .iter()
        .map(|e| e.key)
        .filter(|k| k.contains('.'))
        .collect();
    // Ten dotted keys. `nmp.feed.home` is a NOFS typed sidecar decoded by
    // the hand-written `TypedHomeFeedDecoder` (`swift_reader_type: None`) —
    // not a JSON `SnapshotProjections` field, so it has no `XCTAssertNotNil`
    // in the Swift conformance test, but it IS a dotted registry key.
    let expected = [
        "nmp.nip29.group_events",
        "nmp.nip29.discovered_groups",
        "nmp.nip29.group_defaults",
        "nmp.nip17.dm_inbox",
        "nmp.follow_list",
        "nmp.nip17.dm_relay_list",
        "refs.event.envelopes",
        "nmp.marmot.snapshot",
        "nmp.marmot.messages",
        "nmp.feed.home",
    ];
    for key in expected {
        assert!(
            dotted.contains(&key),
            "expected dotted key {key:?} not in registry"
        );
    }
    // Equal lengths + the forward-contains prove set equality.
    assert_eq!(
        dotted.len(),
        expected.len(),
        "dotted keys drifted: {dotted:?}"
    );
}

/// Drift/overlap guard (ADR-0063 codegen-time partition): a projection key must
/// live in EXACTLY ONE of the two registries — the whole-value
/// `SNAPSHOT_PROJECTIONS` or the keyed `KEYED_PROJECTIONS`. A key in BOTH would
/// drive contradictory generators (a JSON `SnapshotProjections` field AND a
/// per-key row cache for the same key); a keyed projection appearing in NEITHER
/// would silently generate no host cache at all. The keyed projection keys are
/// `refs.*` and must never collide with a `SNAPSHOT_PROJECTIONS` json_key or its
/// typed-sidecar key.
#[test]
fn keyed_and_snapshot_registries_are_disjoint() {
    let snapshot_keys: std::collections::BTreeSet<&str> =
        SNAPSHOT_PROJECTIONS.iter().map(|e| e.key).collect();
    let mut seen_keyed = std::collections::BTreeSet::new();
    for entry in KEYED_PROJECTIONS {
        assert!(
            seen_keyed.insert(entry.projection_key),
            "duplicate projection_key {:?} in KEYED_PROJECTIONS",
            entry.projection_key
        );
        assert!(
            !snapshot_keys.contains(entry.projection_key),
            "keyed projection {:?} also appears in SNAPSHOT_PROJECTIONS — a key must \
             live in exactly one registry (whole-value OR keyed), never both",
            entry.projection_key
        );
    }
}

/// Coverage gate: every entry in the registry MUST carry a typed FlatBuffer
/// sidecar (`typed_sidecar: Some(...)`). A `None` means the projection is a
/// JSON-era vestigial with no typed wire form — such entries must be removed
/// from the registry. The #1610 sweep deleted the five that existed
/// (`timeline`, `inserted`, `updated`, `removed`, `last_action_result`).
///
/// `swift_reader_type: None` inside a `Some(TypedSidecar)` is acceptable:
/// the typed wire exists but the `flatc --swift` binding is not yet
/// checked into the Chirp target. Only `typed_sidecar: None` is banned.
///
/// To add a new sidecar-less entry, you MUST: (a) open a GitHub issue
/// naming the typed-sidecar owner and delivery deadline, and (b) add that
/// issue number to the explicit allowlist below. This makes exemptions
/// durable and auditable rather than silently accumulating.
#[test]
fn typed_sidecar_coverage_gate() {
    // Allowlist for entries that are INTENTIONALLY `typed_sidecar: None`
    // pending a named issue / ADR:
    //   (empty — all five original exemptions removed in #1610)
    const ALLOWED_SIDECAR_LESS: &[&str] = &[];

    for entry in SNAPSHOT_PROJECTIONS {
        if entry.typed_sidecar.is_none() && !ALLOWED_SIDECAR_LESS.contains(&entry.key) {
            panic!(
                "registry entry {:?} has typed_sidecar: None without an approved \
                 exemption. Either add a typed FlatBuffer sidecar for this key OR \
                 open a GitHub issue naming the owner + deadline and add the key to \
                 ALLOWED_SIDECAR_LESS with the issue number in a comment.",
                entry.key
            );
        }
    }
}
