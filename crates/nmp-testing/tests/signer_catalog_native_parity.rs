//! #1493 P9 — the native known-signer detection lists must match the single
//! Rust-owned source of truth (`nmp_core::signer_catalog`).
//!
//! Before this gate the only enforcement was `VendorDriftGateTest` asserting the
//! native Kotlin copies were byte-identical to *each other* — it never tied any
//! of them back to Rust, so the Rust `signer_apps_table()` (now catalog-derived)
//! and the native lists drifted freely (e.g. the `nostrsigner` scheme was
//! labelled "Nostr Signer" in Rust but "Amber" natively). This test closes that
//! gap: it parses the canonical native lists, in order, and asserts each entry
//! matches the corresponding catalog entry offered on that platform. Rust is the
//! single writer; the native lists are conformant mirrors.
//!
//! Order matters — the catalog order is detection precedence (first installed
//! wins), so the comparisons use ordered `Vec`s, not sets. (PR2b will GENERATE
//! the native lists from the catalog and retire this hand-parse gate.)

use std::path::{Path, PathBuf};

use nmp_core::signer_catalog::known_signer_apps;

/// Repo root — `CARGO_MANIFEST_DIR` is `crates/nmp-testing`; the root is two up.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/nmp-testing has a grandparent (repo root)")
        .to_path_buf()
}

fn read(rel: &str) -> String {
    let path = repo_root().join(rel);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {path:?}: {e}"))
}

// ── Catalog expectations (ordered) ───────────────────────────────────────────

/// `(label, scheme, package, content_authority, install_hint)` per Android row.
type AndroidRow = (String, String, String, Option<String>, String);

fn catalog_android() -> Vec<AndroidRow> {
    known_signer_apps()
        .iter()
        .filter_map(|a| {
            a.android.map(|s| {
                (
                    a.display_label.to_string(),
                    s.intent_scheme.to_string(),
                    s.package_name.to_string(),
                    s.content_authority.map(str::to_string),
                    s.install_hint.to_string(),
                )
            })
        })
        .collect()
}

/// `(label, scheme)` per iOS row.
fn catalog_ios() -> Vec<(String, String)> {
    known_signer_apps()
        .iter()
        .filter_map(|a| a.ios.map(|s| (a.display_label.to_string(), s.url_scheme.to_string())))
        .collect()
}

// ── Native parsing ───────────────────────────────────────────────────────────

/// Parse the canonical Kotlin `KNOWN_NOSTR_SIGNERS` list into ORDERED rows.
fn parse_kotlin_known_signers() -> Vec<AndroidRow> {
    let src = read(
        "apps/nmp-gallery/android/app/src/main/kotlin/org/nmp/gallery/registry/ExternalSignerWire.kt",
    );
    let body = kotlin_listof_body(&src, "val KNOWN_NOSTR_SIGNERS");
    body.split("NostrSignerInfo(")
        .skip(1)
        .map(|entry| {
            (
                quoted_after(entry, "displayName").expect("Kotlin entry has displayName"),
                quoted_after(entry, "intentScheme").expect("Kotlin entry has intentScheme"),
                quoted_after(entry, "packageName").expect("Kotlin entry has packageName"),
                kotlin_nullable_string(entry, "contentAuthority"),
                quoted_after(entry, "installHint").expect("Kotlin entry has installHint"),
            )
        })
        .collect()
}

/// Parse a Swift `knownSigners` array into ORDERED `(label, scheme)` rows.
fn parse_swift_known_signers(rel: &str) -> Vec<(String, String)> {
    let src = read(rel);
    let body = swift_known_signers_array_body(&src);
    body.split("NostrSignerInfo(")
        .skip(1)
        .map(|entry| {
            let label = quoted_after(entry, "displayName").expect("Swift entry has displayName");
            // Scheme: explicit `.generic(name:…, scheme: "x")`, or the well-known
            // `.amber` / `.primal` cases whose scheme the catalog also owns.
            let scheme = quoted_after(entry, "scheme")
                .or_else(|| entry.contains(".amber").then(|| "nostrsigner".to_string()))
                .or_else(|| entry.contains(".primal").then(|| "primal".to_string()))
                .unwrap_or_else(|| panic!("Swift entry has no resolvable scheme: {entry}"));
            (label, scheme)
        })
        .collect()
}

// ── Tiny structural parsers ──────────────────────────────────────────────────

/// Body of a Kotlin `<decl> … listOf( … )` initializer — text between the `(`
/// after `listOf` and its depth-matched `)`.
fn kotlin_listof_body<'a>(src: &'a str, decl: &str) -> &'a str {
    let start = src.find(decl).unwrap_or_else(|| panic!("{decl} present"));
    let list = src[start..].find("listOf(").expect("listOf( present") + start + "listOf(".len();
    let bytes = src[list..].as_bytes();
    let mut depth = 1usize;
    for (i, &b) in bytes.iter().enumerate() {
        match b {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return &src[list..list + i];
                }
            }
            _ => {}
        }
    }
    panic!("unterminated listOf( for {decl}");
}

/// Body of the Swift `knownSigners` array literal — text between the `[` that
/// follows the `=` (NOT the `[NostrSignerInfo]` type annotation) and its `]`.
fn swift_known_signers_array_body(src: &str) -> String {
    let decl = src
        .find("static let knownSigners")
        .expect("Swift knownSigners declaration present");
    let eq = src[decl..].find('=').expect("knownSigners has an initializer") + decl;
    let open = src[eq..].find('[').expect("knownSigners array opener") + eq;
    let close = src[open..].find(']').expect("knownSigners array terminator") + open;
    src[open + 1..close].to_string()
}

/// The double-quoted value that follows the first `key` in `haystack` (matches
/// `key = "x"` and `key: "x"`). Stops at the next `"` so it never crosses lines.
fn quoted_after(haystack: &str, key: &str) -> Option<String> {
    let after = &haystack[haystack.find(key)? + key.len()..];
    let q1 = after.find('"')?;
    let rest = &after[q1 + 1..];
    let q2 = rest.find('"')?;
    Some(rest[..q2].to_string())
}

/// A Kotlin `key = null` field → `None`; `key = "x"` → `Some(x)`.
fn kotlin_nullable_string(entry: &str, key: &str) -> Option<String> {
    let after = &entry[entry.find(key)? + key.len()..];
    let eq = after.find('=')?;
    let val = after[eq + 1..].trim_start();
    if val.starts_with("null") {
        None
    } else {
        quoted_after(val, "")
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[test]
fn kotlin_known_signers_match_rust_catalog_android() {
    let native = parse_kotlin_known_signers();
    let catalog = catalog_android();
    assert_eq!(
        native, catalog,
        "Kotlin KNOWN_NOSTR_SIGNERS drifted from nmp_core::signer_catalog (Android), \
         comparing (label, scheme, package, contentAuthority, installHint) IN ORDER.\n\
         native: {native:?}\ncatalog: {catalog:?}\n\
         Rust is the single source of truth — update the catalog, not the native list."
    );
}

#[test]
fn swift_known_signers_match_rust_catalog_ios() {
    let native = parse_swift_known_signers("apps/nmp-gallery/ios/NmpGallery/Registry/NostrLoginBlock.swift");
    let catalog = catalog_ios();
    assert_eq!(
        native, catalog,
        "Swift knownSigners drifted from nmp_core::signer_catalog (iOS), comparing \
         (label, scheme) IN ORDER.\nnative: {native:?}\ncatalog: {catalog:?}\n\
         Rust is the single source of truth — update the catalog, not the native list."
    );
}

/// The VendorDriftGate enforces Kotlin byte-identity but does NOT cover the
/// Swift copies — assert the cli-registry Swift list matches the gallery
/// canonical so the two iOS copies can't silently diverge.
#[test]
fn swift_registry_copy_signer_list_matches_gallery_canonical() {
    let canonical =
        parse_swift_known_signers("apps/nmp-gallery/ios/NmpGallery/Registry/NostrLoginBlock.swift");
    let copy = parse_swift_known_signers("crates/nmp-cli/registry/swiftui/login-block/NostrLoginBlock.swift");
    assert_eq!(
        canonical, copy,
        "cli-registry Swift knownSigners drifted from the gallery canonical"
    );
}
