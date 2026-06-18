# Codex review — #1493 P4 Android wallet is_connected

## Finding
Android `WalletScreen.kt` derived `isConnected = walletTone != null && walletTone != "inactive"`
— a native branch on the Rust `status_tone` wire discriminant (D7 violation). iOS already
gates on the Rust-computed `WalletStatus.is_connected` bool (`WalletView.swift`).

## Fix
Bind the Rust `is_connected` flag (already present in the FlatBuffers schema and the generated
`nmp.nip47.WalletStatus` Kotlin binding) verbatim through `TypedWalletDecoder` →
`SnapshotProjections.walletIsConnected` → `WalletScreen`. Removes the Kotlin tone branch.

## Codex verdict: APPROVE (doctrine-correct), with one intended behavior change
For the wallet **error** state the old and new logic diverge:
- OLD: `tone("error") = "error" != "inactive"` ⇒ Android treated an errored wallet as CONNECTED
  (showed the connected card + "Disconnect Wallet" button).
- NEW: Rust `is_connected` is true only for `connecting`/`ready` ⇒ an errored wallet is NOT
  connected (shows the NWC connect form), matching iOS `status.isConnected`.

This is the correct cross-platform behavior — the old Android branch was the bug. Documented
here per the "never commit reviews into source, promote to durable note" rule; the divergence
is intended and brings Android to iOS parity.
