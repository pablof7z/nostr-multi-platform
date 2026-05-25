//! Thin CLI: regenerate the committed `ContentTreeWire` golden files.
//!
//! Usage: `cargo run -p nmp-content-fixtures --bin build-wire-fixtures`
//! Writes one pretty-JSON file per scenario id under
//! `crates/nmp-content-fixtures/fixtures/wire/<id>.json` (path computed
//! against this crate's `CARGO_MANIFEST_DIR`, so the working directory does
//! not matter).
//!
//! The pinning test (`tests/wire_fixtures.rs`) reads the same files and
//! asserts byte-for-byte equality with a freshly regenerated wire tree. If
//! it fails, the wire contract changed: rerun this binary and commit the
//! diff (after auditing the change is intentional).

use std::fs;
use std::process::ExitCode;

use nmp_content_fixtures::wire_fixtures::{
    build_wire_fixtures, fixture_path_for, serialize_fixture, WIRE_FIXTURE_DIR,
};

fn main() -> ExitCode {
    let fixtures = build_wire_fixtures();

    // Make sure the fixture directory exists. `fixture_path_for` puts every
    // file in the same parent, so creating it once via the first scenario's
    // parent is sufficient — but we do it explicitly here so an empty bundle
    // would still produce the directory layout.
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(WIRE_FIXTURE_DIR);
    if let Err(e) = fs::create_dir_all(&dir) {
        eprintln!("create {} failed: {e}", dir.display());
        return ExitCode::FAILURE;
    }

    let mut written = 0;
    for fx in &fixtures {
        let path = fixture_path_for(&fx.id);
        let body = match serialize_fixture(&fx.wire) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("serialize {} failed: {e}", fx.id);
                return ExitCode::FAILURE;
            }
        };
        if let Err(e) = fs::write(&path, body) {
            eprintln!("write {} failed: {e}", path.display());
            return ExitCode::FAILURE;
        }
        written += 1;
    }

    println!(
        "wrote {written} wire fixtures -> {}",
        dir.display()
    );
    ExitCode::SUCCESS
}
