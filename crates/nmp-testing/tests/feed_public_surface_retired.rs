//! #1740 step 9 — the NEGATIVE matrix: the raw feed lanes are NO LONGER public.
//!
//! Steps 7+8 made `nmp_app_open_feed` / `nmp_app_close_feed` the ONE public
//! app-facing way to open a feed and retired the raw lanes. This grep-gate
//! asserts the retirement is REAL — the retired public symbols/strings are GONE
//! from the public surface, not merely shadowed by a parallel path. It fails the
//! build if any reappears, so a future change cannot silently re-expose them.
//!
//! What is asserted GONE (public surface only):
//!   * the `nmp_app_open_contact_feed` / `nmp_app_close_contact_feed` C-ABI
//!     symbols (no `#[no_mangle]` definition, no `pub use` re-export anywhere);
//!   * the Chirp-specific `nmp_app_chirp_open/close_{home,author,thread}_feed`
//!     production symbols and callers;
//!   * the raw wasm feed-verb dispatch STRINGS (`nmp.kernel.open_interest`,
//!     `nmp.kernel.close_interest`, `nmp.feed.declare_active_follows`,
//!     `nmp.feed.clear_active_follows`) — no routed `action_type` arm.
//!
//! What is INTENTIONALLY still allowed (so this gate does not over-reach):
//!   * `nmp_app_open_interest` / `nmp_app_close_interest` — a generic low-level
//!     interest seam still used by non-feed callers (avatar/uri resolution); its
//!     feed-lane retirement is tracked separately in #1740 (see PR notes).
//!   * comments/docs that NAME a retired symbol to document its removal.
//!
//! # Running
//!
//! ```bash
//! cargo test -p nmp-testing --test feed_public_surface_retired
//! ```

use std::fs;
use std::path::{Path, PathBuf};

/// Repo root (CARGO_MANIFEST_DIR is `crates/nmp-testing`).
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repo root is two levels above crates/nmp-testing")
        .to_path_buf()
}

/// Collect every `.rs` file under `dir`, skipping `target/` build artifacts.
fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "target") {
                continue;
            }
            rust_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// A code line that is NOT a `//` comment (we allow retired symbols to be NAMED
/// in comments documenting their removal). Returns the code portion only.
fn code_only(line: &str) -> &str {
    let trimmed = line.trim_start();
    if trimmed.starts_with("//") {
        return "";
    }
    line
}

#[test]
fn contact_feed_c_abi_symbols_are_not_defined_or_reexported() {
    // The retired C-ABI shims must not be DEFINED (`#[no_mangle] ... fn name`)
    // or RE-EXPORTED (`pub use ... name`) anywhere. A bare mention in a comment
    // is allowed (documents the removal); a code-line occurrence is a violation.
    const RETIRED_C_SYMBOLS: &[&str] = &[
        "nmp_app_open_contact_feed",
        "nmp_app_close_contact_feed",
        "nmp_app_chirp_open_home_feed",
        "nmp_app_chirp_close_home_feed",
        "nmp_app_chirp_open_author_feed",
        "nmp_app_chirp_close_author_feed",
        "nmp_app_chirp_open_thread_feed",
        "nmp_app_chirp_close_thread_feed",
    ];

    let root = repo_root();
    let mut files = Vec::new();
    for sub in ["crates", "apps"] {
        rust_files(&root.join(sub), &mut files);
    }
    assert!(!files.is_empty(), "must scan some Rust sources");

    let mut violations = Vec::new();
    for file in &files {
        // This grep-gate's own source NAMES the retired symbols (the literal it
        // searches for); skip it so the gate does not flag itself.
        if file.ends_with("tests/feed_public_surface_retired.rs") {
            continue;
        }
        let Ok(text) = fs::read_to_string(file) else {
            continue;
        };
        for (n, line) in text.lines().enumerate() {
            let code = code_only(line);
            for sym in RETIRED_C_SYMBOLS {
                if code.contains(sym) {
                    violations.push(format!("{}:{}: {}", file.display(), n + 1, line.trim()));
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "retired contact-feed C-ABI symbols reappeared in the public surface \
         (must be DELETED, not re-exposed):\n{}",
        violations.join("\n")
    );
}

#[test]
fn raw_wasm_feed_verb_dispatch_strings_are_not_routed() {
    // The raw public wasm feed-verb action strings must not be routed by any
    // `action_type` match arm. We assert they do not appear in a NON-comment
    // line under crates/nmp-wasm/src (the dispatch router). A doc/comment naming
    // the retired string is allowed.
    const RETIRED_DISPATCH_STRINGS: &[&str] = &[
        "nmp.kernel.open_interest",
        "nmp.kernel.close_interest",
        "nmp.feed.declare_active_follows",
        "nmp.feed.clear_active_follows",
    ];

    let wasm_src = repo_root().join("crates").join("nmp-wasm").join("src");
    let mut files = Vec::new();
    rust_files(&wasm_src, &mut files);
    assert!(!files.is_empty(), "must scan nmp-wasm sources");

    let mut violations = Vec::new();
    for file in &files {
        // The router's own tests file is excluded — but step 8 deleted those
        // tests, so this is belt-and-suspenders; a routed arm in any non-comment
        // line is the violation.
        let Ok(text) = fs::read_to_string(file) else {
            continue;
        };
        for (n, line) in text.lines().enumerate() {
            let code = code_only(line);
            for s in RETIRED_DISPATCH_STRINGS {
                if code.contains(s) {
                    violations.push(format!("{}:{}: {}", file.display(), n + 1, line.trim()));
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "retired raw wasm feed-verb dispatch strings reappeared in the router \
         (the public open_feed doorway is the only feed-open surface):\n{}",
        violations.join("\n")
    );
}

#[test]
fn public_open_feed_doorway_symbols_exist() {
    // The POSITIVE companion: the ONE public doorway must be DEFINED. This guards
    // against an over-zealous future cleanup deleting the replacement along with
    // the retired lanes (which would leave NO public way to open a feed).
    let feed_rs = repo_root().join("apps/chirp/crates/nmp-app-chirp/src/ffi/feed.rs");
    let text = fs::read_to_string(&feed_rs)
        .unwrap_or_else(|e| panic!("the public feed doorway file must exist: {e}"));
    for sym in ["nmp_app_open_feed", "nmp_app_close_feed"] {
        assert!(
            text.contains(&format!("pub extern \"C\" fn {sym}")),
            "the public feed doorway `{sym}` must be defined in {}",
            feed_rs.display()
        );
    }
}
