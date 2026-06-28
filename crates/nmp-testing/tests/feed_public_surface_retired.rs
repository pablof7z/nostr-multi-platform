//! #1740 step 9 — the NEGATIVE matrix: the raw feed lanes are NO LONGER public.
//!
//! Feed public-surface migration retired the raw lanes and the older app-local
//! feed FFI exports. This grep-gate asserts the retirement is real: retired
//! public symbols/strings are gone from the public surface, not merely shadowed
//! by a parallel path. It fails if any reappears, so a future change cannot
//! silently re-expose them.
//!
//! What is asserted GONE (public surface only):
//!   * the old feed C-ABI symbols (no `#[no_mangle]` definition, no `pub use`
//!     re-export anywhere);
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
use std::process::Command;

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

/// Collect git-tracked source files in `subdirs` (relative to `root`) that
/// have a relevant public-surface extension.  Shells out to `git ls-files` so
/// gitignored generated output (e.g. `web/chirp/dist/`, `nmp-browser-runtime/`
/// wasm artifact dirs) is automatically excluded — no directory-skip list is
/// needed for ignored paths.
fn git_tracked_surface_files(root: &Path, subdirs: &[&str], out: &mut Vec<PathBuf>) {
    let mut cmd = Command::new("git");
    cmd.arg("ls-files")
        .arg("--")
        .args(subdirs)
        .current_dir(root);
    let output = cmd
        .output()
        .expect("git ls-files must succeed for public-surface scan");
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let rel: &Path = Path::new(line);
        if rel.extension().is_some_and(|ext| {
            matches!(
                ext.to_str(),
                Some("rs" | "swift" | "kt" | "kts" | "ts" | "tsx" | "js" | "json")
            )
        }) {
            out.push(root.join(rel));
        }
    }
}

fn binary_contains(bytes: &[u8], needle: &[u8]) -> bool {
    bytes.windows(needle.len()).any(|window| window == needle)
}

/// A code line that is NOT a `//` comment (we allow retired symbols to be NAMED
/// in comments documenting their removal). Returns the code portion only.
fn code_only(line: &str) -> &str {
    let trimmed = line.trim_start();
    if trimmed.starts_with("//") || trimmed.starts_with("/*") || trimmed.starts_with('*') {
        return "";
    }
    line
}

#[test]
fn checked_in_native_libs_do_not_export_contact_feed_symbols() {
    // The source scan below is not enough: the gallery Android JNI library is
    // tracked as a prebuilt `.so`, and a stale binary can keep exporting a
    // deleted public symbol. Scan bytes directly so the gate does not depend on
    // platform-specific `nm` availability.
    const RETIRED_C_SYMBOLS: &[&str] = &[
        "nmp_app_open_contact_feed",
        "nmp_app_close_contact_feed",
        "claimed_event_embeds",
    ];

    let root = repo_root();
    let libs =
        [root
            .join("apps/nmp-gallery/android/app/src/main/jniLibs/arm64-v8a/libnmp_app_gallery.so")];

    let mut violations = Vec::new();
    for lib in libs {
        let bytes = fs::read(&lib).unwrap_or_else(|err| {
            panic!(
                "{} must be readable for public-surface scan: {err}",
                lib.display()
            )
        });
        for sym in RETIRED_C_SYMBOLS {
            if binary_contains(&bytes, sym.as_bytes()) {
                violations.push(format!("{} contains {sym}", lib.display()));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "retired public-surface strings remain in checked-in native libs:\n{}",
        violations.join("\n")
    );
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
    // `action_type` match arm. nmp-wasm was deleted in #2202; we now scan
    // crates/ and apps/ broadly to guard against re-routing. A doc/comment
    // naming the retired string is allowed.
    const RETIRED_DISPATCH_STRINGS: &[&str] = &[
        "nmp.kernel.open_interest",
        "nmp.kernel.close_interest",
        "nmp.feed.declare_active_follows",
        "nmp.feed.clear_active_follows",
    ];

    // nmp-wasm was deleted in #2202. Now scan the full crates/ and apps/ trees
    // to guard against the retired strings being routed anywhere.
    let root = repo_root();
    let mut files = Vec::new();
    for sub in ["crates", "apps"] {
        rust_files(&root.join(sub), &mut files);
    }
    assert!(!files.is_empty(), "must scan some Rust sources");

    let mut violations = Vec::new();
    for file in &files {
        // Skip this gate file itself — it NAMES the retired strings as the
        // literals it searches for, so it would self-flag.
        if file.ends_with("tests/feed_public_surface_retired.rs") {
            continue;
        }
        let file_text = file.to_string_lossy().replace('\\', "/");
        if file_text.contains("/crates/nmp-testing/bin/doctrine-lint/") {
            continue;
        }
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
        "retired raw wasm feed-verb dispatch strings reappeared in the codebase \
         (feed product screens must not route retired raw feed verbs; \
         nmp-wasm was deleted in #2202 and these strings must not be re-routed):\n{}",
        violations.join("\n")
    );
}

#[test]
fn claimed_event_embeds_key_is_not_used_in_public_surfaces() {
    let root = repo_root();
    let mut files = Vec::new();
    git_tracked_surface_files(&root, &["crates", "apps", "web"], &mut files);
    assert!(!files.is_empty(), "must scan public source surfaces");

    let mut violations = Vec::new();
    for file in &files {
        if file.ends_with("tests/feed_public_surface_retired.rs") {
            continue;
        }
        let Ok(text) = fs::read_to_string(file) else {
            continue;
        };
        for (n, line) in text.lines().enumerate() {
            if code_only(line).contains("claimed_event_embeds") {
                violations.push(format!("{}:{}: {}", file.display(), n + 1, line.trim()));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "retired public projection key `claimed_event_embeds` reappeared in a \
         production/public surface; use `refs.event.envelopes` derived from \
         authoritative `refs.event` rows instead:\n{}",
        violations.join("\n")
    );
}

#[test]
fn old_public_open_feed_doorway_symbols_are_not_defined_or_reexported() {
    // Chirp's in-repo app-local feed FFI was removed. Do not resurrect those
    // old C symbols as a replacement for the typed-session direction.
    const RETIRED_FEED_SYMBOLS: &[&str] = &["nmp_app_open_feed", "nmp_app_close_feed"];

    let root = repo_root();
    let mut files = Vec::new();
    for sub in ["crates", "apps"] {
        rust_files(&root.join(sub), &mut files);
    }
    assert!(!files.is_empty(), "must scan some Rust sources");

    let mut violations = Vec::new();
    for file in &files {
        if file.ends_with("tests/feed_public_surface_retired.rs") {
            continue;
        }
        let Ok(text) = fs::read_to_string(file) else {
            continue;
        };
        for (n, line) in text.lines().enumerate() {
            let code = code_only(line);
            for sym in RETIRED_FEED_SYMBOLS {
                if code.contains(sym) {
                    violations.push(format!("{}:{}: {}", file.display(), n + 1, line.trim()));
                }
            }
        }
    }

    assert!(
        violations.is_empty(),
        "retired open_feed C-ABI symbols reappeared in code; typed sessions must \
         replace this surface rather than restoring the old app-local feed FFI:\n{}",
        violations.join("\n")
    );
}
