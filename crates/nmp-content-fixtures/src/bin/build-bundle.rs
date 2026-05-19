//! Thin CLI: `build_bundle()` → pretty JSON at the fixed bundle path.
//!
//! Usage: `cargo run -p nmp-content-fixtures --bin build-bundle`
//! (run from the workspace root). Writes
//! `ios/NmpGallery/Resources/content-gallery-bundle.json`.

use std::fs;
use std::path::Path;
use std::process::ExitCode;

use nmp_content_fixtures::{build_bundle, BUNDLE_PATH};

fn main() -> ExitCode {
    let bundle = build_bundle();
    let json = match serde_json::to_string_pretty(&bundle) {
        Ok(j) => j,
        Err(e) => {
            eprintln!("serialize bundle failed: {e}");
            return ExitCode::FAILURE;
        }
    };

    let path = Path::new(BUNDLE_PATH);
    if let Some(parent) = path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            eprintln!("create {} failed: {e}", parent.display());
            return ExitCode::FAILURE;
        }
    }
    if let Err(e) = fs::write(path, format!("{json}\n")) {
        eprintln!("write {BUNDLE_PATH} failed: {e}");
        return ExitCode::FAILURE;
    }

    println!(
        "wrote {} scenarios -> {BUNDLE_PATH}",
        bundle.scenarios.len()
    );
    ExitCode::SUCCESS
}
