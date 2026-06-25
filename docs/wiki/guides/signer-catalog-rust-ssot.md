---
title: Signer Catalog Rust SSOT
slug: signer-catalog-rust-ssot
topic: ffi-runtime
summary: The known signer apps list must have Rust as the single source of truth
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-18
updated: 2026-06-19
verified: 2026-06-18
compiled-from: conversation
sources:
  - session:019edc01-fdde-7b20-a348-5a2a9ce1a0f9
  - session:11850f79-923f-4a2a-a921-a4b9bec47c6c
  - session:019edc3e-b4a1-72a0-b791-9dcfdd615785
  - session:019edc4d-4175-7441-b5af-cb2012068335
  - session:019edc7a-0321-7b52-afbf-804eff9d280c
  - session:019edc84-6e5c-74a2-9ed9-57938dae31a1
  - session:019edc94-e2f8-76e3-8cdc-a6d8f6bba72a
  - session:019edcc8-fa82-7c13-9736-ecf1337bc58c
---

# Signer Catalog Rust SSOT

## Source of Truth

The known signer apps list must have Rust as the single source of truth. Known-signers source of truth is a Rust-owned catalog struct (KnownSignerApp with app_id, display_label, capabilities as a set of SignerCapability such as Nip55/Nip46, and per-platform detection specs android: Option<AndroidSpec> and ios: Option<IosSpec>). The Android detection spec contains intent_scheme, package_name, optional content_authority, and install_hint; the iOS detection spec contains url_scheme and install_hint. None means not offered on that platform. The catalog must reside in a low-dependency module or crate usable by both nmp-core and nmp-codegen. Native manifests and plists are codegen'd from the Rust catalog rather than hand-maintained independently; native login-block detection lists (Surface B) must be generated from the Rust catalog via nmp-codegen rather than consumed via runtime projection, because Android package visibility and iOS URL-scheme probing require compile-time manifest/plist declarations. nmp-codegen must be extended with a signer-catalog generator that emits Kotlin known-signer constants, Swift known-signer constants, TypeScript known-signer constants, Android query declarations (or checked generated manifest fragments), and iOS query-scheme declarations (or checked generated plist fragments). The Rust catalog is the single writer (D4) for known-signer data; native surfaces derive mechanically from it via `nmp gen signer-catalog`, enforced by the codegen-drift CI gate. Both the NIP-46 onboarding projection (Surface A) and native login-block detection lists (Surface B) must derive from this unified Rust catalog, rather than maintaining separate Surface A and Surface B catalogs. The existing signer_apps_table() in identity.rs must be refactored to derive from the unified Rust catalog rather than owning its own table. VendorDriftGate must be reworked into a `--check` gate asserting native artifacts match the Rust signer catalog digest (not just native-to-native parity), ensuring Rust-to-native drift is caught by the codegen-drift CI gate. No native domain logic (aim.md §2 #4); if an if statement in Swift, Kotlin, or any native language decides what the app should do (not how it should look), that logic belongs in Rust. Native code may only render UI, execute OS capabilities, or hold ephemeral presentation state — never decide policy, retry, or cache. Thin native shells (Swift/Kotlin) must hold no operator data. relay_diagnostics emits only raw tokens; its *_tone hue selectors were removed (#1802) — shells derive color from the raw tokens. dump_signer_catalog_json() uses unwrap_or_default() (matching codegen_schema precedent) as the D6-compliant no-panic fallback for serializing the catalog. The public static let knownSigners in the cli-registry Swift template must remain nonisolated (not @MainActor) since it is pure catalog data; detect()/canOpen() stay @MainActor. Adding @MainActor to the pure catalog static is source-breaking for downstream users who read it from nonisolated code. P4 Finding 4 (ExternalSignerCapabilityBridge transport + concurrent-Intent rejection) is not a violation; transport selection is mechanical from Rust-set fields and concurrent-Intent rejection is an OS Activity-Result launcher capacity constraint.

<!-- citations: [^11850-108] [^11850-62] [^11850-63] [^11850-64] [^019ed-25] [^019ed-26] [^019ed-27] [^019ed-28] [^019ed-29] [^019ed-30] [^11850-19] [^019ed-83] [^11850-44] [^11850-84] [^019ed-102] [^11850-106] [^019ed-112] [^019ed-115] [^11850-125] [^11850-151] [^11850-175] [^11850-246] [^11850-255] -->
## Signer Modeling

Scheme alone is not identity; the same nostrsigner:// scheme served by Amber on Android (NIP-55) and potentially a generic NIP-46 bridge elsewhere requires distinct catalog entries with distinct capabilities and per-platform specs, not one row with a platform-dependent label. Known-signer labels are converged to the Rust catalog: nostrsigner→'Amber' (as NIP-55), nostrconnect→'Nostr Connect'. (Previously: the nostrsigner entry was labeled 'Nostr Signer' in Rust.) Rust-to-native drift is enforced by a `--check` gate replacing the former native-only VendorDriftGate parity. The known-signers model uses exactly ONE nostrsigner entry = Amber (app_id=amber, display_label=Amber, capabilities=[Nip55, Nip46], both platforms), dropping the generic 'Nostr Signer' row; nostrconnect becomes 'Nostr Connect'. The known-signer catalog entry for Primal is app_id=primal, display_label=Primal, capabilities=[Nip46], android intent_scheme=primal + package_name + content_authority + install_hint, iOS URL scheme=primal + install_hint. The known-signer catalog entry for Nostr Connect is app_id=nostr_connect, display_label=Nostr Connect, capabilities=[Nip46], android=None (valid omission), iOS URL scheme=nostrconnect + install_hint. The iOS NIP-46 onboarding probe table (signer_apps_table()) is derived from the signer catalog by filtering for entries where ios is Some and capabilities include Nip46, producing scheme + '://', display_label, and signer_kind nip46. Amber is correctly excluded from the iOS NIP-46 onboarding table because its ios is None (Android-only). On Chirp NIP-46 onboarding (Surface A) on iOS, nostrconnect:// renders 'Nostr Connect'. On the login-block detection list (Surface B), nostrsigner renders 'Amber' on Android. The current Rust signer_kind = 'nip46' for every row is incorrect; capabilities must be modeled as a set of SignerCapability (e.g., Amber as Nip55+Nip46, Primal as Nip46, Nostr Connect as Nip46). Labels are converged across surfaces: nostrsigner → 'Amber', nostrconnect → 'Nostr Connect'. The signer_state projection emits raw semantic tokens (signer_kind, connection_state, stage); shells render labels via shared parity-consistent helper mappings, NOT from Rust precomputed English labels — ADR-0032/#1099's label precompute pattern is reversed for signer_state.

<!-- citations: [^019ed-31] [^11850-20] [^11850-45] [^11850-85] [^019ed-103] [^11850-107] [^019ed-111] [^019ed-116] [^11850-126] [^11850-152] [^11850-176] [^11850-200] [^11850-247] [^11850-256] -->
## Cross-Platform Drift Cases

chirpConfig.ts relay role has drifted from Rust: Rust says 'both' while TS says 'both,indexer' — a concrete divergence confirmed by P4 verification. nmp-chirp-config role values must match the Rust source single-source-of-truth; the confirmed drift must be resolved. P4 Finding 6 (chirpConfig.ts relay defaults) is absorbed into P9's PR1 vertical — nmp-chirp-config is the single source of truth and generates the TS list; the confirmed drift is resolved to match Rust. The TypeScript relay config should eventually be generated from the Rust source of truth (apps/chirp/crates/nmp-chirp-config/src/lib.rs) to prevent config drift from recurring; this is assigned to the p9/core-config lane.

P4 Finding 2 (Android WalletScreen `isConnected`) is fixed in PR #1530: Android now binds the Rust-computed `WalletStatus.is_connected` bool verbatim instead of deriving connection state from the tone discriminant, correcting errored wallets wrongly showing a Disconnect button.

P4 Finding 3 (SignInScreen signerKind label switch) is folded into the P9 labels-to-shells lane (Direction A: Rust emits raw signer_kind token, shells render via shared helper), NOT by adding a Rust precompute. ADR-0032/#1099's label precompute pattern is reversed for signer_state: Rust emits raw semantic tokens (signer_kind, connection_state, stage) and shells map token→label via a shared, parity-consistent helper.

P9 has full vertical ownership for the three coupled breaking changes (relays/pubkeys, known-signers, signer-labels), absorbing P4 Finding 3 and Finding 6.

<!-- citations: [^11850-21] [^11850-22] [^019ed-88] [^11850-86] [^11850-105] [^11850-127] [^11850-153] [^11850-177] [^11850-199] [^11850-248] -->
## PR2 Scope and Constraints

P9's known-signers work is phased: PR2a ships the Rust catalog + nip46_onboarding derivation + label convergence + Rust-tied native↔Rust parity gate; PR2b does full codegen of Kotlin×3/Swift×2 lists + AndroidManifest <queries> + iOS Info.plist LSApplicationQueriesSchemes, retiring the hand-parse gate. A gate-only intermediate is acceptable but NOT the final state — codegen is mandatory. The native signer list is an embedded ~110-line section inside triple-vendored multi-purpose files and Swift copies already drift, making full codegen essential.

The signer-catalog codegen emits Kotlin KnownSigners.generated.kt (3 vendored copies differing only by package line) and Swift KnownSigners.generated.swift (2 copies) from the dump_signer_catalog JSON on stdin, plus a --check drift gate that asserts AndroidManifest <queries> schemes and Info.plist LSApplicationQueriesSchemes match the catalog.

The current native parity gate (signer_catalog_native_parity.rs) asserts that parsed native Kotlin and Swift signer lists match the Rust catalog's per-platform entries as BTreeSets of (display_label, scheme). However, using unordered BTreeSets allows reorder drift to pass even though catalog ordering is detection precedence; ordered Vecs should be used for parity to preserve precedence. Additionally, the Android parity gate only checks (display_label, intentScheme) and does not verify package_name, content_authority, or install_hint, allowing those fields to drift from the Rust catalog while still passing.

The manifest/plist extraction functions can false-pass on commented-out XML: commented-out <data android:scheme> or <string> elements inside the scoped block would still count as present, so the CI gate can pass even when the actual query declaration is missing. Generated Kotlin and Swift string literals are not escaped: display_label, content_authority, install_hint, url_scheme values containing double quotes, backslashes, or newlines would produce invalid or semantically changed source code, and --check would still consider the invalid render authoritative.

<!-- citations: [^019ed-104] [^11850-109] [^019ed-117] [^11850-128] [^019ed-141] [^11850-154] [^11850-178] [^11850-201] [^11850-221] [^11850-233] -->
