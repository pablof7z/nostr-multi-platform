//! #1493 P9 — generate the native known-signer detection lists from the
//! Rust-owned catalog (`nmp_core::signer_catalog`).
//!
//! PR2a landed the single Rust source of truth (the `KNOWN_SIGNER_APPS` slice +
//! the `dump_signer_catalog` binary that prints it as JSON) and a hand-parse
//! parity gate. PR2b (this module) makes `nmp-codegen` GENERATE the native list
//! literals from that JSON so they are exact-by-construction and can never
//! drift, then retires the hand-parse gate.
//!
//! Generated outputs (one `val KNOWN_NOSTR_SIGNERS` / `knownSigners` literal
//! split out of the larger hand-authored wire files):
//!   * Kotlin `KnownSigners.generated.kt` — two byte-identical-except-package
//!     copies (gallery canonical + the CLI install registry), holding the
//!     Android entries in catalog order.
//!   * Swift `KnownSigners.generated.swift` — two byte-identical copies (gallery
//!     canonical + the CLI install registry), holding the iOS entries as an
//!     `extension NostrSignerDetector { static let knownSigners }`.
//!
//! Additionally CHECK-only (never clobbered — this is a large hand file with
//! unrelated content): the `AndroidManifest <queries>` intent schemes must
//! stay in sync with the catalog's Android schemes. The iOS `Info.plist`
//! scheme check lives in the external Chirp repo, which manages its own plist.
//!
//! `nmp-codegen` intentionally has NO `nmp-core` dependency (only serde /
//! serde_json). This module parses the catalog JSON into a LOCAL typed struct,
//! mirroring the `rust_builtin_keys` / `swift` modules' posture, so the codegen
//! crate stays kernel-free.

use std::path::{Path, PathBuf};

use serde::Deserialize;

// ── Local typed mirror of the `dump_signer_catalog` JSON shape ────────────────
//
// Deliberately a separate struct from `nmp_core::signer_catalog::KnownSignerApp`
// so `nmp-codegen` need not depend on `nmp-core`. The field names match the
// serde wire shape exactly; `content_authority` / `android` / `ios` are omitted
// from the JSON when `None` (the Rust side uses `skip_serializing_if`), so they
// default to `None` here.

/// One known external signer app as decoded from the catalog JSON.
#[derive(Debug, Deserialize)]
pub struct SignerApp {
    pub app_id: String,
    pub display_label: String,
    // `allow(dead_code)`: decoded from the JSON catalog for schema completeness;
    // presence is validated in tests but the Rust side does not consume the list.
    #[allow(dead_code)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub android: Option<AndroidSpec>,
    #[serde(default)]
    pub ios: Option<IosSpec>,
}

/// Android detection mechanics for one signer app.
#[derive(Debug, Deserialize)]
pub struct AndroidSpec {
    pub intent_scheme: String,
    pub package_name: String,
    #[serde(default)]
    pub content_authority: Option<String>,
}

/// iOS detection mechanics for one signer app.
#[derive(Debug, Deserialize)]
pub struct IosSpec {
    pub url_scheme: String,
}

// ── Output targets (relative to the repo root, the codegen cwd) ───────────────
//
// The binary runs from the workspace root, so these relative paths resolve
// against the checkout root both in CI and locally.

/// A Kotlin generated-file target: its on-disk path + the `package` line it must
/// carry (VendorDriftGate enforces byte-identity across copies EXCEPT line 1).
struct KotlinTarget {
    path: &'static str,
    package: &'static str,
}

const KOTLIN_TARGETS: &[KotlinTarget] = &[
    // Gallery canonical (VendorDriftGate canonical).
    KotlinTarget {
        path: "apps/nmp-gallery/android/app/src/main/kotlin/org/nmp/gallery/registry/KnownSigners.generated.kt",
        package: "org.nmp.gallery.registry",
    },
    // The CLI install registry copy (vendored into consumer apps).
    KotlinTarget {
        path: "crates/nmp-cli/registry/compose/login-block/KnownSigners.generated.kt",
        package: "org.nmp.registry",
    },
];

const SWIFT_TARGETS: &[&str] = &[
    // Gallery canonical.
    "apps/nmp-gallery/ios/NmpGallery/Registry/KnownSigners.generated.swift",
    // CLI install registry copy.
    "crates/nmp-cli/registry/swiftui/login-block/KnownSigners.generated.swift",
];

const ANDROID_MANIFEST: &str = "apps/nmp-gallery/android/app/src/main/AndroidManifest.xml";

// ── Generated-file headers ────────────────────────────────────────────────────

const KOTLIN_HEADER_BODY: &str = "\
// ─────────────────────────────────────────────────────────────────────────────
// THIS FILE IS GENERATED. DO NOT EDIT BY HAND.
//
// Source of truth: nmp_core::signer_catalog (crates/nmp-core/src/signer_catalog.rs).
// Regenerate via:
//   cargo run -q -p nmp-core --bin dump_signer_catalog \\
//     | cargo run -q -p nmp-codegen -- gen signer-catalog
//
// The CI gate (.github/workflows/codegen-drift.yml, `nmp gen signer-catalog
// --check`) fails any PR whose generated native signer lists differ from a fresh
// run, so they can never drift from the Rust catalog.
// ─────────────────────────────────────────────────────────────────────────────

/**
 * Ordered list of signers the Android detector knows about (detection
 * precedence = list order). Consumed by `detectInstalledSigners` in
 * ExternalSignerWire.kt (same package). Every `intentScheme` here MUST also
 * appear in `<queries>` in AndroidManifest.xml.
 */
";

const SWIFT_HEADER: &str = "\
// ─────────────────────────────────────────────────────────────────────────────
// THIS FILE IS GENERATED. DO NOT EDIT BY HAND.
//
// Source of truth: nmp_core::signer_catalog (crates/nmp-core/src/signer_catalog.rs).
// Regenerate via:
//   cargo run -q -p nmp-core --bin dump_signer_catalog \\
//     | cargo run -q -p nmp-codegen -- gen signer-catalog
//
// The CI gate (.github/workflows/codegen-drift.yml, `nmp gen signer-catalog
// --check`) fails any PR whose generated native signer lists differ from a fresh
// run, so they can never drift from the Rust catalog.
// ─────────────────────────────────────────────────────────────────────────────

";

// ── Catalog parsing ───────────────────────────────────────────────────────────

/// Parse the `dump_signer_catalog` JSON (a top-level array) into typed apps.
///
/// # Errors
/// Returns the serde error string if the JSON is not a `Vec<SignerApp>`.
pub fn parse_catalog(catalog_json: &str) -> Result<Vec<SignerApp>, String> {
    serde_json::from_str(catalog_json)
        .map_err(|e| format!("failed to parse signer catalog JSON: {e}"))
}

// ── String-literal escaping ───────────────────────────────────────────────────

/// Escape a catalog string for embedding in a double-quoted Kotlin/Swift string
/// literal. Both languages share the `\"` / `\\` escapes; a stray newline is
/// escaped to `\n` so the generated source stays a single valid literal. Today's
/// catalog strings are plain ASCII (no-op), but a future entry with a quote must
/// not produce invalid generated source that `--check` would then bless.
fn esc(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n")
}

// ── Kotlin rendering ──────────────────────────────────────────────────────────

/// Render the `KnownSigners.generated.kt` source for the given catalog and
/// `package` line. Emits the Android entries (apps with an `android` spec), in
/// catalog order, into the `KNOWN_NOSTR_SIGNERS` literal.
#[must_use]
pub fn render_kotlin_known_signers(apps: &[SignerApp], package: &str) -> String {
    let mut out = String::new();
    out.push_str("package ");
    out.push_str(package);
    out.push_str("\n\n");
    out.push_str(KOTLIN_HEADER_BODY);
    out.push_str("val KNOWN_NOSTR_SIGNERS: List<NostrSignerInfo> = listOf(\n");
    for app in apps {
        let Some(android) = &app.android else { continue };
        let content_authority = match &android.content_authority {
            Some(c) => format!("\"{}\"", esc(c)),
            None => "null".to_string(),
        };
        out.push_str("    NostrSignerInfo(\n");
        out.push_str(&format!("        displayName = \"{}\",\n", esc(&app.display_label)));
        out.push_str(&format!("        intentScheme = \"{}\",\n", esc(&android.intent_scheme)));
        out.push_str(&format!("        contentAuthority = {content_authority},\n"));
        out.push_str(&format!("        packageName = \"{}\",\n", esc(&android.package_name)));
        out.push_str("    ),\n");
    }
    out.push_str(")\n");
    out
}

// ── Swift rendering ───────────────────────────────────────────────────────────

/// Render the `KnownSigners.generated.swift` source. Emits the iOS entries
/// (apps with an `ios` spec), in catalog order, as an
/// `extension NostrSignerDetector { static let knownSigners }`.
///
/// `app_id == "amber"` → `.amber`, `"primal"` → `.primal`; anything else →
/// `.generic(name: <label>, scheme: <ios.url_scheme>)`, matching the typed
/// `SignerKind` cases the hand-written `NostrLoginBlock.swift` declares.
#[must_use]
pub fn render_swift_known_signers(apps: &[SignerApp]) -> String {
    let mut out = String::new();
    out.push_str(SWIFT_HEADER);
    out.push_str("extension NostrSignerDetector {\n\n");
    out.push_str("    /// Ordered list of signers this detector knows about (detection precedence\n");
    out.push_str("    /// = array order). Every `urlScheme` here MUST also appear in Info.plist's\n");
    out.push_str("    /// `LSApplicationQueriesSchemes`.\n");
    // `@MainActor` placement matches the original hand-written declaration; the
    // member stays `public` to preserve the existing API surface (it was
    // `public static let` in the type body).
    out.push_str("    @MainActor\n");
    out.push_str("    public static let knownSigners: [NostrSignerInfo] = [\n");
    for app in apps {
        let Some(ios) = &app.ios else { continue };
        match app.app_id.as_str() {
            "amber" => out.push_str(&format!(
                "        NostrSignerInfo(kind: .amber, displayName: \"{}\"),\n",
                esc(&app.display_label)
            )),
            "primal" => out.push_str(&format!(
                "        NostrSignerInfo(kind: .primal, displayName: \"{}\"),\n",
                esc(&app.display_label)
            )),
            _ => {
                out.push_str("        NostrSignerInfo(\n");
                out.push_str(&format!(
                    "            kind: .generic(name: \"{}\", scheme: \"{}\"),\n",
                    esc(&app.display_label),
                    esc(&ios.url_scheme)
                ));
                out.push_str(&format!("            displayName: \"{}\"\n", esc(&app.display_label)));
                out.push_str("        ),\n");
            }
        }
    }
    out.push_str("    ]\n");
    out.push_str("}\n");
    out
}

// ── Manifest / plist scheme extraction (CHECK only) ───────────────────────────

/// The catalog's Android intent schemes, in order (apps with an `android` spec).
fn android_schemes(apps: &[SignerApp]) -> Vec<String> {
    apps.iter()
        .filter_map(|a| a.android.as_ref().map(|s| s.intent_scheme.clone()))
        .collect()
}

/// Extract the substring between the first `open` and the next `close` marker,
/// or `None` if either is absent. Used to scope scheme scanning to the relevant
/// manifest `<queries>` / plist array block so unrelated schemes never match.
fn between<'a>(haystack: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = haystack.find(open)? + open.len();
    let end = haystack[start..].find(close)? + start;
    Some(&haystack[start..end])
}

/// Remove XML/plist comments (`<!-- … -->`) so a commented-out scheme cannot
/// satisfy the presence check — otherwise a `<!-- <data android:scheme="primal"
/// /> -->` would false-pass while the real declaration is missing.
fn strip_xml_comments(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    let mut rest = src;
    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        match rest[start..].find("-->") {
            Some(end) => rest = &rest[start + end + "-->".len()..],
            None => return out, // unterminated comment — drop the remainder
        }
    }
    out.push_str(rest);
    out
}

/// All `android:scheme="X"` values inside the `<queries>` block, in order.
fn manifest_query_schemes(manifest: &str) -> Vec<String> {
    let manifest = strip_xml_comments(manifest);
    let block = match between(&manifest, "<queries>", "</queries>") {
        Some(b) => b,
        None => return Vec::new(),
    };
    collect_attr_values(block, "android:scheme=\"")
}


/// Collect every double-quoted value following each occurrence of `attr` (e.g.
/// `android:scheme="`), in order.
fn collect_attr_values(haystack: &str, attr: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = haystack;
    while let Some(i) = rest.find(attr) {
        let after = &rest[i + attr.len()..];
        if let Some(q) = after.find('"') {
            out.push(after[..q].to_string());
            rest = &after[q + 1..];
        } else {
            break;
        }
    }
    out
}


// ── Generate / check orchestration ────────────────────────────────────────────

/// Outcome of a `--check` run across all generated files + the manifest/plist
/// scheme assertions. `problems` is a human-readable line per stale/mismatched
/// surface; empty ⇒ everything is in sync.
#[derive(Debug)]
pub struct SignerCatalogCheckOutcome {
    pub up_to_date: bool,
    pub problems: Vec<String>,
}

/// Generate ALL native signer-list files from the catalog JSON. The
/// manifest/plist are CHECK-only and are NOT written here (they are large hand
/// files with unrelated content).
///
/// Returns the list of written paths.
///
/// # Errors
/// JSON parse failures or filesystem I/O failures.
pub fn generate_signer_catalog(catalog_json: &str) -> Result<Vec<PathBuf>, String> {
    let apps = parse_catalog(catalog_json)?;
    let mut written = Vec::new();
    for target in KOTLIN_TARGETS {
        let rendered = render_kotlin_known_signers(&apps, target.package);
        write_file(Path::new(target.path), &rendered)?;
        written.push(PathBuf::from(target.path));
    }
    let swift = render_swift_known_signers(&apps);
    for path in SWIFT_TARGETS {
        write_file(Path::new(path), &swift)?;
        written.push(PathBuf::from(path));
    }
    Ok(written)
}

/// `--check`: diff every generated file against a fresh render and assert the
/// manifest/plist schemes match the catalog. A missing file, a content diff, or
/// a scheme mismatch each becomes a `problems` entry.
///
/// # Errors
/// JSON parse failures or filesystem I/O failures other than NotFound.
pub fn check_signer_catalog(catalog_json: &str) -> Result<SignerCatalogCheckOutcome, String> {
    let apps = parse_catalog(catalog_json)?;
    let mut problems = Vec::new();

    for target in KOTLIN_TARGETS {
        let expected = render_kotlin_known_signers(&apps, target.package);
        check_file(target.path, &expected, &mut problems)?;
    }
    let swift = render_swift_known_signers(&apps);
    for path in SWIFT_TARGETS {
        check_file(path, &swift, &mut problems)?;
    }

    check_schemes(
        ANDROID_MANIFEST,
        &android_schemes(&apps),
        manifest_query_schemes,
        "AndroidManifest <queries> intent schemes",
        &mut problems,
    )?;

    Ok(SignerCatalogCheckOutcome {
        up_to_date: problems.is_empty(),
        problems,
    })
}

/// Write `contents` to `path`, creating parent dirs.
fn write_file(path: &Path, contents: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("creating {}: {e}", parent.display()))?;
    }
    std::fs::write(path, contents).map_err(|e| format!("writing {}: {e}", path.display()))
}

/// Diff `expected` against the file at `rel_path`, pushing a `problems` entry on
/// drift or absence.
fn check_file(rel_path: &str, expected: &str, problems: &mut Vec<String>) -> Result<(), String> {
    match std::fs::read_to_string(rel_path) {
        Ok(actual) => {
            if actual != expected {
                let where_diff = crate::diff_report::first_diff_or_length(&actual, expected)
                    .map(|n| format!(" (first differing line {n})"))
                    .unwrap_or_default();
                problems.push(format!("{rel_path}: stale generated file{where_diff}"));
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            problems.push(format!("{rel_path}: missing generated file"));
        }
        Err(e) => return Err(format!("reading {rel_path}: {e}")),
    }
    Ok(())
}

/// Assert the schemes extracted from `rel_path` (via `extract`) equal the
/// `expected` catalog schemes, in order.
fn check_schemes(
    rel_path: &str,
    expected: &[String],
    extract: fn(&str) -> Vec<String>,
    label: &str,
    problems: &mut Vec<String>,
) -> Result<(), String> {
    let src = match std::fs::read_to_string(rel_path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            problems.push(format!("{rel_path}: missing ({label})"));
            return Ok(());
        }
        Err(e) => return Err(format!("reading {rel_path}: {e}")),
    };
    let found = extract(&src);
    if found != expected {
        problems.push(format!(
            "{rel_path}: {label} drifted from catalog.\n    found:    {found:?}\n    expected: {expected:?}"
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "signer_catalog/tests.rs"]
mod tests;
