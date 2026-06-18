# Codex review — #1493 P9 PR2b (signer-catalog codegen)

Date: 2026-06-19. codex (gpt-5-codex). Branch: fix/1493-p9-signer-codegen.

nmp-codegen `gen signer-catalog [--check]` reads the dump_signer_catalog JSON (stdin) and GENERATES the native Kotlin KNOWN_NOSTR_SIGNERS (×3 copies) + Swift knownSigners (×2) list literals into KnownSigners.generated.{kt,swift} siblings, plus asserts AndroidManifest <queries> / iOS plist LSApplicationQueriesSchemes contain the catalog schemes. CI step added to codegen-drift.yml; PR2a hand-parse gate retired; VendorDriftGate extended to the generated Kotlin file.

## Findings (both LOW, non-live; hardened anyway)
1. Manifest/plist scheme check could false-pass on a COMMENTED-OUT `<data android:scheme="…"/>` / `<string>…</string>`. FIXED: strip_xml_comments() removes `<!-- … -->` before extraction.
2. Generated Kotlin/Swift string literals were interpolated unescaped — a future catalog string with `"`/`\`/newline would emit invalid source that --check would bless. FIXED: esc() escapes `\` `"` `\n` on every interpolated value. (No-op for the current ASCII catalog — generated output byte-identical.)

## Clean (codex-verified)
Omitted content_authority/android/ios parse as None; Kotlin emits `contentAuthority = null`; Swift maps amber/primal→typed cases, else→.generic; --check covers all 5 files + manifest/plist and treats a missing file as failure; scheme comparison is order-sensitive exact equality; no unordered map/set iteration (deterministic output).

Verified post-fix: cargo build -p nmp-codegen clean; gen signer-catalog --check idempotent (byte-identical); cargo test -p nmp-codegen signer_catalog 5/5; cargo test -p nmp-cli --test export 3/3; doctrine_lint_smoke 78/78; cargo test -p nmp-app-chirp green (#1553 CI blind spot covered).
