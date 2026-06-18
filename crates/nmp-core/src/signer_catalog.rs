//! The Rust-owned **single source of truth** for the known external Nostr
//! signer apps (#1493 P9).
//!
//! Before this module there were two un-reconciled catalogs:
//!   * Surface A — the NIP-46 `nostrconnect` onboarding probe table
//!     (`signer_apps_table()` in `actor/commands/identity.rs`), shipped in the
//!     `nip46_onboarding` projection and consumed by the iOS onboarding screen.
//!   * Surface B — the native login-block "known signers" lists
//!     (`KNOWN_NOSTR_SIGNERS` in Kotlin, `knownSigners` in Swift, and the TS
//!     equivalents) used for local NIP-55 / NIP-46 app detection
//!     (`PackageManager.queryIntentActivities` / `UIApplication.canOpenURL`).
//!
//! The two drifted (e.g. the `nostrsigner` scheme was labelled "Nostr Signer"
//! in Rust but "Amber" natively) and the only gate that existed enforced
//! byte-identity *between the native copies*, never tying any of them back to
//! Rust. This catalog is the one writer; every other surface is **generated
//! from** or **derived from** it:
//!   * `actor::commands::identity::signer_apps_table()` derives Surface A.
//!   * `nmp-codegen` renders the native Kotlin/Swift/TS lists + the
//!     `AndroidManifest <queries>` / iOS `LSApplicationQueriesSchemes` scheme
//!     declarations from the JSON this module exports (`dump_signer_catalog`).
//!
//! ## Modelling note (codex design)
//!
//! Scheme alone is not identity. The same `nostrsigner://` scheme is served by
//! a specific Android NIP-55 vendor app (Amber) AND could be a generic NIP-46
//! bridge elsewhere — those are distinct catalog entries with distinct
//! capabilities and per-platform specs, not one row with a platform-dependent
//! label. So each entry carries:
//!   * protocol/product facts Rust owns: `app_id`, `display_label`,
//!     `capabilities` (which of NIP-55 / NIP-46 it speaks);
//!   * per-platform detection *mechanics* (still Rust-owned data, executed by
//!     the shell): `android` (Intent scheme + package + ContentProvider
//!     authority) and `ios` (URL scheme). `None` means "not offered on this
//!     platform" — e.g. the Nostr Connect bridge is iOS-only (no Android row),
//!     while Amber and Primal are offered on both.
//!
//! This module is intentionally dependency-light (only `serde`) so the
//! `dump_signer_catalog` binary and `nmp-codegen` can share the exact shape
//! without pulling in kernel internals.

use serde::Serialize;

/// A signing protocol a known app speaks. An app may speak more than one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SignerCapability {
    /// NIP-55 — Android Intent-based external signing (e.g. Amber).
    Nip55,
    /// NIP-46 — remote signing over a relay (`bunker://` / `nostrconnect://`).
    Nip46,
}

impl SignerCapability {
    /// Stable wire token (matches the serde rename) for codegen / projections.
    #[must_use]
    pub const fn as_token(self) -> &'static str {
        match self {
            SignerCapability::Nip55 => "nip55",
            SignerCapability::Nip46 => "nip46",
        }
    }
}

/// Android detection mechanics for one signer app.
///
/// `intent_scheme` MUST appear in `AndroidManifest <queries>` (Android 11+ hides
/// otherwise-unqueryable packages) — the codegen emits that block from this
/// field. `package_name` is the APK id used for `PackageManager` lookups;
/// `content_authority` is the optional ContentProvider fast-path namespace
/// (`None` ⇒ Intent round-trip only).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct AndroidSignerSpec {
    pub intent_scheme: &'static str,
    pub package_name: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_authority: Option<&'static str>,
    pub install_hint: &'static str,
}

/// iOS detection mechanics for one signer app.
///
/// `url_scheme` MUST appear in `Info.plist` `LSApplicationQueriesSchemes`
/// (`UIApplication.canOpenURL` returns `false` for undeclared schemes) — the
/// codegen emits that array from this field.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct IosSignerSpec {
    pub url_scheme: &'static str,
    pub install_hint: &'static str,
}

/// One known external signer app — the unit of the catalog.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct KnownSignerApp {
    /// Stable machine id (snake_case, never shown to users). Codegen uses it for
    /// generated identifiers; gates use it as the join key.
    pub app_id: &'static str,
    /// Human-readable name shown in detected-signer CTAs (Rust-owned product
    /// fact — e.g. the vendor brand "Amber", or the generic "Nostr Connect").
    pub display_label: &'static str,
    /// Which signing protocol(s) this app speaks. Never empty.
    pub capabilities: &'static [SignerCapability],
    /// Android detection mechanics, or `None` when not offered on Android.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub android: Option<AndroidSignerSpec>,
    /// iOS detection mechanics, or `None` when not offered on iOS.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ios: Option<IosSignerSpec>,
}

impl KnownSignerApp {
    /// True when this app speaks `capability`.
    #[must_use]
    pub fn speaks(&self, capability: SignerCapability) -> bool {
        self.capabilities.contains(&capability)
    }
}

/// The canonical catalog. THE one writer — every native list, manifest/plist
/// scheme set, and the `nip46_onboarding` projection is generated from or
/// derived from this.
///
/// Ordering is the host detection precedence (first installed wins on each
/// platform).
pub const KNOWN_SIGNER_APPS: &[KnownSignerApp] = &[
    // Amber — the canonical signer behind the `nostrsigner://` scheme on BOTH
    // platforms. Speaks NIP-55 (Android Intent signing) and NIP-46 (remote).
    // Scheme-only detection (canOpenURL / PackageManager) cannot distinguish a
    // second identity behind `nostrsigner://`, so there is exactly ONE entry for
    // that scheme — Amber — never a separate generic "Nostr Signer" row. "Nostr
    // Signer" is reserved for an unknown/generic fallback, never the known
    // `nostrsigner` row (#1493 P9 design).
    KnownSignerApp {
        app_id: "amber",
        display_label: "Amber",
        capabilities: &[SignerCapability::Nip55, SignerCapability::Nip46],
        android: Some(AndroidSignerSpec {
            intent_scheme: "nostrsigner",
            package_name: "com.greenart7c3.nostrsigner",
            content_authority: Some("com.greenart7c3.nostrsigner"),
            install_hint: "Install Amber for one-tap sign-in",
        }),
        ios: Some(IosSignerSpec {
            url_scheme: "nostrsigner",
            install_hint: "Install Amber for one-tap sign-in",
        }),
    },
    // Primal — registers `primal://` on both platforms and acts as a remote
    // (NIP-46) signer. No Android ContentProvider fast-path.
    KnownSignerApp {
        app_id: "primal",
        display_label: "Primal",
        capabilities: &[SignerCapability::Nip46],
        android: Some(AndroidSignerSpec {
            intent_scheme: "primal",
            package_name: "net.primal.android",
            content_authority: None,
            install_hint: "Install Primal for one-tap sign-in",
        }),
        ios: Some(IosSignerSpec {
            url_scheme: "primal",
            install_hint: "Install Primal for one-tap sign-in",
        }),
    },
    // Nostr Connect — the generic NIP-46 bridge reached via the
    // `nostrconnect://` scheme (any compliant remote signer). iOS-only here:
    // Android has no generic NIP-46 bridge entry (it uses Amber for NIP-55).
    KnownSignerApp {
        app_id: "nostr_connect",
        display_label: "Nostr Connect",
        capabilities: &[SignerCapability::Nip46],
        android: None,
        ios: Some(IosSignerSpec {
            url_scheme: "nostrconnect",
            install_hint: "Connect a remote signer",
        }),
    },
];

/// Borrow the canonical catalog.
#[must_use]
pub fn known_signer_apps() -> &'static [KnownSignerApp] {
    KNOWN_SIGNER_APPS
}

/// Serialize the catalog to the pretty JSON document `nmp-codegen` consumes to
/// render the native lists + manifest/plist scheme declarations. The
/// `dump_signer_catalog` binary is a thin shim over this.
///
/// Serialization of this static, plain-`Serialize` data cannot fail in
/// practice; the `unwrap_or_default()` (an empty string) is the D6-compliant
/// no-panic fallback, mirroring `codegen_schema::dump_pilot_schemas_json`.
#[must_use]
pub fn dump_signer_catalog_json() -> String {
    serde_json::to_string_pretty(KNOWN_SIGNER_APPS).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_non_empty_and_well_formed() {
        assert!(!KNOWN_SIGNER_APPS.is_empty());
        for app in KNOWN_SIGNER_APPS {
            assert!(!app.app_id.is_empty(), "app_id must be non-empty");
            assert!(!app.display_label.is_empty(), "display_label must be non-empty");
            assert!(!app.capabilities.is_empty(), "every app speaks ≥1 protocol");
            assert!(
                app.android.is_some() || app.ios.is_some(),
                "{} must be offered on at least one platform",
                app.app_id
            );
            if let Some(android) = app.android {
                assert!(!android.intent_scheme.is_empty());
                assert!(!android.package_name.is_empty());
                assert!(!android.install_hint.is_empty());
            }
            if let Some(ios) = app.ios {
                assert!(!ios.url_scheme.is_empty());
                assert!(!ios.install_hint.is_empty());
            }
        }
    }

    #[test]
    fn app_ids_are_unique() {
        let mut ids: Vec<&str> = KNOWN_SIGNER_APPS.iter().map(|a| a.app_id).collect();
        ids.sort_unstable();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "app_id values must be unique");
    }

    #[test]
    fn dump_json_round_trips_entry_count() {
        let json = dump_signer_catalog_json();
        let parsed: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        assert_eq!(
            parsed.as_array().expect("catalog is a JSON array").len(),
            KNOWN_SIGNER_APPS.len()
        );
    }
}
