# Codex review — #1493 P9 PR3 (signer labels-to-shells)

Date: 2026-06-19. codex (gpt-5-codex). Branch: fix/1493-p9-signer-labels.

Direction A (labels-to-shells, confirmed by team-lead autonomy grant + aim.md:62 + merged siblings #1568/#1536/#1537). Removed status_label/status_tone from signer_state.fbs + stage_label from bunker_handshake.fbs (both were tail-appended last fields → no offset renumbering); deleted signer_state_label.rs + stage_label_for() + the DTO fields; regenerated Rust/Swift bindings (flatc 25.12.19); promoted the existing shell derivation fallbacks (Android deriveStatusLabel/Tone, iOS SignerStateTone) to the primary path; P4 F3 SignInScreen signerKind→rowLabel stays shell-side off the raw token.

## Codex verdict: NO correctness bugs.
- Rust decode safe round-trip: removed fields not read; remaining strings use unwrap_or_default(), no required-field unwrap (signer_state_fb.rs:106, bunker_handshake_fb.rs:90).
- DTO/sidecar construction complete (identity.rs:175, typed_projections/mod.rs:83); "connected"→"ready" canonicalized (identity.rs:198).
- Swift Decodable correct: statusLabel/statusTone/stageLabel are computed extension properties (not stored), so synthesized decoding no longer expects the removed JSON keys (KernelSignerTypes.swift:81).
- SignerStateTone covers ready/connected/reconnecting/awaiting_approval/unavailable/failed + unknown fallback.

## Outstanding (CI verifies)
- Kotlin flatc-drift gate pins flatc 25.2.10 (not installed locally; only 25.12.19). SignerState.kt was hand-edited to remove ONLY the two trailing field accessors+builders (startTable 11→9), preserving 25.2.10 boilerplate. Field-removal is structurally correct (removed fields were last). CI's check-kotlin-flatc-drift.sh (25.2.10) must confirm.
- Native Swift/Kotlin compile + tests verified by CI (no Xcode/Gradle locally).

Verified locally: cargo build -p nmp-core clean; nmp-core signer 49 + typed_projections 35 green; nmp-app-chirp green (#1553); doctrine_lint_smoke 78; rust-flatc-drift + swift-flatc-drift OK.
