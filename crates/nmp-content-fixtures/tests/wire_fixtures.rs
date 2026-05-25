//! Pin the committed `ContentTreeWire` golden files (`fixtures/wire/*.json`)
//! against a freshly regenerated tree for every scenario.
//!
//! Drift means the cross-platform wire contract changed: iOS and Android
//! decoders would see different bytes than the ones the M16 fixture pack
//! ships. The fix is intentional and explicit — audit the change, then
//! regenerate by hand with:
//!
//! ```sh
//! cargo run -p nmp-content-fixtures --bin build-wire-fixtures
//! ```
//!
//! The comparison is **byte-exact** (the committed file's exact bytes vs
//! `serialize_fixture`'s exact bytes) because the bytes themselves *are* the
//! contract. A structural-only comparison would mask formatting drift that
//! a fresh consumer would still see.

use std::fs;

use nmp_content_fixtures::wire_fixtures::{
    build_wire_fixtures, fixture_path_for, serialize_fixture,
};

/// One assertion per scenario id: the committed golden file must match the
/// freshly generated wire bytes, character for character.
#[test]
fn wire_goldens_match() {
    let mut drift = Vec::new();
    for fx in build_wire_fixtures() {
        let path = fixture_path_for(&fx.id);
        let expected = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                drift.push(format!(
                    "missing golden file for {id}: {e} (path: {p})",
                    id = fx.id,
                    p = path.display(),
                ));
                continue;
            }
        };
        let actual = serialize_fixture(&fx.wire).expect(
            "ContentTreeWire is plain serde derive; serialization is infallible",
        );
        if expected != actual {
            drift.push(format!(
                "wire contract changed for {id}: update the golden file by \
                 running `cargo run -p nmp-content-fixtures --bin \
                 build-wire-fixtures` (path: {p})",
                id = fx.id,
                p = path.display(),
            ));
        }
    }
    assert!(
        drift.is_empty(),
        "{n} wire-contract drift(s):\n  - {body}",
        n = drift.len(),
        body = drift.join("\n  - "),
    );
}

/// Defensive: every committed golden must correspond to a known scenario id.
/// Catches the inverse of `wire_goldens_match` — a stale file left over
/// from a deleted scenario.
#[test]
fn every_golden_file_belongs_to_a_scenario() {
    let known: std::collections::BTreeSet<String> = build_wire_fixtures()
        .into_iter()
        .map(|f| f.id)
        .collect();
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(nmp_content_fixtures::wire_fixtures::WIRE_FIXTURE_DIR);
    let mut orphans = Vec::new();
    let entries = fs::read_dir(&dir).expect("wire fixture dir exists; \
        run `cargo run -p nmp-content-fixtures --bin build-wire-fixtures`");
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        let Some(id) = name.strip_suffix(".json") else {
            continue;
        };
        if !known.contains(id) {
            orphans.push(name.to_string());
        }
    }
    assert!(
        orphans.is_empty(),
        "orphan wire fixture(s) in {}: {:?} — \
         either restore the scenario or `git rm` the file",
        dir.display(),
        orphans,
    );
}
