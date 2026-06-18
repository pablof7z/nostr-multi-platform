# Codex design — #1493 P9 PR2 (known-signers Rust source of truth)

Date: 2026-06-18. codex (gpt-5-codex). Branch: fix/1493-p9-known-signers.

## Problem
Two signer surfaces with drift, only native↔native gated:
- Surface A: Rust signer_apps_table() → nip46_onboarding FlatBuffers projection (iOS Chirp consumes via canOpenURL probing; Android/web don't).
- Surface B: native login-block hardcoded lists (KNOWN_NOSTR_SIGNERS Kotlin / knownSigners Swift / TS) for local NIP-55/Amber detection. VendorDriftGate enforces byte-identity native↔native, never tied to Rust.
Drift: nostrsigner labeled "Nostr Signer" (Rust) vs "Amber" (native); nostrconnect "Signer App" (Rust) / "Nostr Signer" (Swift) / absent (Android); native carries packageName/contentAuthority Rust lacks.

## Verdict (decisive — overrides "keep separate")
ONE Rust catalog drives BOTH surfaces:
```
KnownSignerApp { app_id, display_label, capabilities:[Nip55|Nip46],
  android: Option<AndroidSpec { intent_scheme, package_name, content_authority, install_hint }>,
  ios: Option<IosSpec { url_scheme, install_hint }> }
```
- The Amber-vs-NostrSigner distinction is REAL but mis-modeled: Amber = NIP-55 Android app behind nostrsigner:// (android-only entry, capability Nip55); a generic NIP-46 nostrsigner bridge would be a SEPARATE entry. Android omitting nostrconnect is valid via android:None. nostrconnect label → "Nostr Connect".
- Mechanism = CODEGEN (not runtime, not gate-only): native detection needs compile-time AndroidManifest <queries> + iOS plist LSApplicationQueriesSchemes, so the catalog must GENERATE the native lists + manifest/plist. Keep detector mechanics (canOpenURL / PackageManager) intact.
- Place the catalog in a low-dep Rust location usable by nmp-core AND nmp-codegen. Per repo pattern: nmp-core owns the catalog + a `dump_signer_catalog` binary (JSON, gated behind codegen-schema); nmp-codegen reads the JSON and renders Kotlin/Swift/TS + manifest/plist via `nmp gen signer-catalog [--check]`, CI-gated in codegen-drift.yml.
- Refactor signer_apps_table()/nip46_onboarding to DERIVE from the catalog (NIP-46-capable entries with an ios spec). Replace VendorDriftGate native↔native with the Rust-catalog --check gate.

## PR2 minimum scope (codex)
Add catalog + correct labels/capabilities + derive nip46_onboarding from it + generate (or Rust-check) the native login-block lists AND manifest/plist schemes from it + keep detector mechanics. Do NOT rewrite shell detection flows. Gate-only (no codegen) is acceptable only as an INTERMEDIATE, not the final state (would leave two writers).

## Model refinement (codex follow-up)
Conflict found: two iOS surfaces both use nostrsigner:// — onboarding labeled it "Nostr Signer", login-block labeled it "Amber". Codex verdict: scheme-only detection (canOpenURL/PackageManager) cannot distinguish two identities behind one scheme, so there is exactly ONE catalog entry for nostrsigner = Amber. "Nostr Signer" is reserved for an unknown/generic fallback, never the known row. Dropped nostrsigner_generic.

FINAL catalog (3 entries):
- amber: "Amber", [Nip55, Nip46], android(nostrsigner, pkg com.greenart7c3.nostrsigner, authority com.greenart7c3.nostrsigner), ios(nostrsigner)
- primal: "Primal", [Nip46], android(primal, pkg net.primal.android), ios(primal)
- nostr_connect: "Nostr Connect", [Nip46], android None, ios(nostrconnect)

Surface A (Chirp onboarding, iOS, NIP-46 entries): nostrsigner→Amber, primal→Primal, nostrconnect→Nostr Connect.
Surface B (login-block detection): iOS = Amber, Primal, Nostr Connect; Android = Amber, Primal.

WEB is excluded from codegen: it detects window.nostr NIP-07 only (kind:"nip07"), no app catalog.

## Codegen targets (Surface B)
- Kotlin KNOWN_NOSTR_SIGNERS + NostrSignerInfo: section of ExternalSignerWire.kt, ×3 vendored copies (gallery canonical, android/app live, cli-registry compose). Generate the list section into a generated sibling per copy.
- Swift knownSigners + NostrSignerInfo: section of NostrLoginBlock.swift, ×2 copies (gallery, cli-registry swiftui).
- AndroidManifest <queries> (android-spec schemes: nostrsigner, primal).
- iOS Info.plist LSApplicationQueriesSchemes (ios-spec schemes: nostrsigner, primal, nostrconnect).
Mechanism: nmp gen signer-catalog [--check] reading dump_signer_catalog JSON; CI gate in codegen-drift.yml. VendorDriftGate reworked so the generated section is checked against the Rust catalog, hand-authored remainder stays native↔native.
