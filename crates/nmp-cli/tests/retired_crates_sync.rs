//! Verifies `src/doctor/retired_crates.toml` (vendored, published with the
//! crate) stays in sync with `release/nmp-release.toml`'s `[[retired_crates]]`
//! (canonical source). This test reaches outside the crate directory via
//! `CARGO_MANIFEST_DIR`, which is only valid within the workspace checkout --
//! safe here because `cargo publish` builds the package without running
//! tests.

use std::collections::BTreeMap;
use std::path::Path;

fn parse_retired_crates(text: &str) -> BTreeMap<String, String> {
    let value: toml::Value = text.parse().expect("valid TOML");
    let mut out = BTreeMap::new();
    if let Some(items) = value.get("retired_crates").and_then(toml::Value::as_array) {
        for item in items {
            let name = item
                .get("name")
                .and_then(toml::Value::as_str)
                .expect("retired_crates entry missing name")
                .to_string();
            let migration = item
                .get("migration")
                .and_then(toml::Value::as_str)
                .expect("retired_crates entry missing migration")
                .to_string();
            out.insert(name, migration);
        }
    }
    out
}

#[test]
fn vendored_retired_crates_matches_canonical_release_manifest() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));

    let vendored_path = manifest_dir.join("src/doctor/retired_crates.toml");
    let vendored = std::fs::read_to_string(&vendored_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", vendored_path.display()));

    let canonical_path = manifest_dir.join("../../release/nmp-release.toml");
    let canonical = std::fs::read_to_string(&canonical_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", canonical_path.display()));

    let vendored_entries = parse_retired_crates(&vendored);
    let canonical_entries = parse_retired_crates(&canonical);

    assert_eq!(
        vendored_entries, canonical_entries,
        "crates/nmp-cli/src/doctor/retired_crates.toml has drifted from \
         release/nmp-release.toml's [[retired_crates]] -- update the vendored \
         copy (it must stay in sync since published crates can't reach the \
         workspace root file)"
    );
    assert!(!canonical_entries.is_empty(), "sanity: retired_crates should be non-empty");
}
