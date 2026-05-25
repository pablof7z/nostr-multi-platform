//! `ContentTreeWire` golden-fixture generator.
//!
//! Every scenario produced by [`crate::build_bundle`] carries its primary
//! signed event (with the exact `kind`, `tags`, and `content` the tokenizer
//! needs). This module re-runs the **real** `nmp_content::tokenize_with_kind`
//! over each primary event and calls `ContentTree::to_wire()` to produce the
//! canonical FFI wire form (`ContentTreeWire`) that Swift / Kotlin decoders
//! must parse. The output is a stable JSON document per scenario id.
//!
//! ### Boundary
//!
//! These golden files are the **wire contract**. They are committed under
//! `crates/nmp-content-fixtures/fixtures/wire/<scenario-id>.json` and pinned
//! by an integration test ([`tests::wire_goldens_match`]). Drift means the
//! cross-platform contract changed and the generator must be re-run by hand
//! (`cargo run -p nmp-content-fixtures --bin build-wire-fixtures`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use nmp_content::{tokenize_with_kind, ContentTreeWire, RenderMode};

use crate::build_bundle;
use crate::dto::SignedEventJson;

/// One generated wire-form fixture entry.
pub struct WireFixture {
    /// Scenario id (e.g. `S-T01`).
    pub id: String,
    /// The `ContentTreeWire` for the scenario's primary event.
    pub wire: ContentTreeWire,
}

/// Path (relative to this crate's `Cargo.toml`) of the wire golden directory.
pub const WIRE_FIXTURE_DIR: &str = "fixtures/wire";

/// Re-run the tokenizer over every scenario's primary event and project to
/// the canonical [`ContentTreeWire`] arena. The primary event is the first
/// entry in [`crate::dto::ScenarioDto::events`] (see `scenarios::scenario`).
///
/// Returns one entry per scenario, in `build_bundle()` order. Scenario ids
/// are unique (asserted by `tests/bundle.rs::bundle_has_expected_scenario_count`).
#[must_use]
pub fn build_wire_fixtures() -> Vec<WireFixture> {
    let bundle = build_bundle();
    bundle
        .scenarios
        .into_iter()
        .map(|s| {
            let primary = s
                .events
                .first()
                .expect("every scenario carries its primary event first");
            let wire = wire_for_event(primary);
            WireFixture { id: s.id, wire }
        })
        .collect()
}

/// Project one signed-event's content through the real tokenizer to its
/// `ContentTreeWire` form. Pure; no relay / network / store touched.
#[must_use]
pub fn wire_for_event(ev: &SignedEventJson) -> ContentTreeWire {
    let tree = tokenize_with_kind(
        &ev.content,
        &ev.tags,
        RenderMode::Auto,
        ev.kind,
    );
    tree.to_wire()
}

/// Serialize one [`WireFixture`] to its committed golden-file form
/// (pretty JSON + trailing newline). This is the **exact** byte form on
/// disk; the pinning test does a byte-equality comparison against this.
///
/// Returns `Err` only if `serde_json` fails to format — structurally
/// impossible for `ContentTreeWire` (every variant is plain derive), but the
/// generator is explicit rather than panicking (D6).
pub fn serialize_fixture(wire: &ContentTreeWire) -> Result<String, serde_json::Error> {
    let mut s = serde_json::to_string_pretty(wire)?;
    s.push('\n');
    Ok(s)
}

/// Absolute path of the committed golden file for a given scenario id,
/// computed relative to this crate's `Cargo.toml` (so the path is stable
/// regardless of where `cargo run` / `cargo test` was invoked from).
#[must_use]
pub fn fixture_path_for(id: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(WIRE_FIXTURE_DIR)
        .join(format!("{id}.json"))
}

/// Build the full id -> wire map. Convenience for tests that want random
/// lookup (the pinning test uses [`build_wire_fixtures`] directly so it
/// preserves iteration order).
#[must_use]
pub fn build_wire_fixture_map() -> BTreeMap<String, ContentTreeWire> {
    build_wire_fixtures()
        .into_iter()
        .map(|f| (f.id, f.wire))
        .collect()
}
