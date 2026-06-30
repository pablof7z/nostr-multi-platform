//! Unit tests for the #1493 P9 signer-catalog codegen.

use super::*;

/// The exact JSON shape `dump_signer_catalog` emits (3 apps; Primal omits
/// `content_authority`, Nostr Connect omits `android`).
/// Note: `install_hint` prose is NOT in the catalog JSON (#1681) — that is UI
/// copy owned by shells, not signer identity. Shells format their own hint from
/// `display_label` (e.g. "Install {displayName} for one-tap sign-in").
const CATALOG_JSON: &str = r#"[
  { "app_id": "amber", "display_label": "Amber", "capabilities": ["nip55","nip46"],
    "android": { "intent_scheme": "nostrsigner", "package_name": "com.greenart7c3.nostrsigner",
                 "content_authority": "com.greenart7c3.nostrsigner" },
    "ios": { "url_scheme": "nostrsigner" } },
  { "app_id": "primal", "display_label": "Primal", "capabilities": ["nip46"],
    "android": { "intent_scheme": "primal", "package_name": "net.primal.android" },
    "ios": { "url_scheme": "primal" } },
  { "app_id": "nostr_connect", "display_label": "Nostr Connect", "capabilities": ["nip46"],
    "ios": { "url_scheme": "nostrconnect" } }
]"#;

fn apps() -> Vec<SignerApp> {
    parse_catalog(CATALOG_JSON).expect("catalog parses")
}

#[test]
fn parse_handles_omitted_optionals() {
    let apps = apps();
    assert_eq!(apps.len(), 3);
    // Primal omits content_authority → None.
    let primal = &apps[1];
    assert!(primal.android.as_ref().unwrap().content_authority.is_none());
    // Nostr Connect omits android → None; iOS-only.
    let nc = &apps[2];
    assert!(nc.android.is_none());
    assert!(nc.ios.is_some());
}

#[test]
fn kotlin_render_is_deterministic_and_faithful() {
    let a = render_kotlin_known_signers(&apps(), "org.nmp.gallery.registry");
    let b = render_kotlin_known_signers(&apps(), "org.nmp.gallery.registry");
    assert_eq!(a, b, "render must be deterministic");

    assert!(a.starts_with("package org.nmp.gallery.registry\n"));
    // Android entries only, in order: Amber then Primal (Nostr Connect excluded).
    assert!(a.contains("displayName = \"Amber\""));
    assert!(a.contains("contentAuthority = \"com.greenart7c3.nostrsigner\""));
    assert!(
        a.contains("contentAuthority = null"),
        "Primal has null authority"
    );
    assert!(a.contains("packageName = \"net.primal.android\""));
    assert!(
        !a.contains("Nostr Connect"),
        "iOS-only app must not appear in Android list"
    );
    let amber_at = a.find("\"Amber\"").unwrap();
    let primal_at = a.find("\"Primal\"").unwrap();
    assert!(amber_at < primal_at, "catalog order preserved");
}

#[test]
fn kotlin_copies_differ_only_in_package_line() {
    let gallery = render_kotlin_known_signers(&apps(), "org.nmp.gallery.registry");
    let android = render_kotlin_known_signers(&apps(), "org.nmp.android");
    let g: Vec<&str> = gallery.lines().collect();
    let a: Vec<&str> = android.lines().collect();
    assert_eq!(g.len(), a.len());
    assert!(g[0].starts_with("package ") && a[0].starts_with("package "));
    assert_ne!(g[0], a[0], "package line differs");
    assert_eq!(
        &g[1..],
        &a[1..],
        "everything after line 1 is byte-identical"
    );
}

#[test]
fn swift_render_uses_typed_and_generic_kinds() {
    let s = render_swift_known_signers(&apps());
    assert!(s.contains("extension NostrSignerDetector {"));
    assert!(s.contains("@MainActor"));
    assert!(s.contains("public static let knownSigners: [NostrSignerInfo] = ["));
    // amber/primal → typed cases; nostr_connect → generic with its url_scheme.
    assert!(s.contains("NostrSignerInfo(kind: .amber, displayName: \"Amber\"),"));
    assert!(s.contains("NostrSignerInfo(kind: .primal, displayName: \"Primal\"),"));
    assert!(s.contains(".generic(name: \"Nostr Connect\", scheme: \"nostrconnect\")"));
    // Order preserved.
    let amber_at = s.find(".amber").unwrap();
    let primal_at = s.find(".primal").unwrap();
    let generic_at = s.find(".generic").unwrap();
    assert!(amber_at < primal_at && primal_at < generic_at);
}

#[test]
fn manifest_scheme_extraction() {
    let manifest = r#"<manifest>
      <queries>
        <intent><action android:name="android.intent.action.VIEW" /><data android:scheme="nostrsigner" /></intent>
        <intent><action android:name="android.intent.action.VIEW" /><data android:scheme="primal" /></intent>
      </queries>
      <application><activity><intent-filter><data android:scheme="https" /></intent-filter></activity></application>
    </manifest>"#;
    // Only the <queries> block schemes are collected (the https deep-link is ignored).
    assert_eq!(
        manifest_query_schemes(manifest),
        vec!["nostrsigner".to_string(), "primal".to_string()]
    );
}
