---
type: research-record
date: 2026-06-14
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
source_lines: 33-202
agent_attribution: Verify MLS support iOS+Android (sonnet agent)
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "MLS/Marmot support verification for iOS and Android — verdict SHOULD-WORK-UNVERIFIED for both platforms with code-grounded FFI/JNI/UI evidence"
captured_at: 2026-06-14T10:36:55Z
---

MLS/Marmot support verification for iOS and Android — verdict SHOULD-WORK-UNVERIFIED for both platforms with code-grounded FFI/JNI/UI evidence

---

User: [Agent task result: Agent "Verify MLS support iOS+Android" completed]
I have enough information. Let me compile the comprehensive report.

---

## VERDICT

### iOS (Chirp): SHOULD-WORK-UNVERIFIED

**Evidence that the end-to-end flow is plumbed:**

1. **Rust crate** `nmp-marmot` at `crates/nmp-marmot/` is a complete MLS-over-Nostr Layer-4 NIP crate wrapping `mdk-core 0.8.0` + `mdk-sqlite-storage 0.8.0`. All MLS ops (create group, invite, join, send, receive) are implemented in Rust.

2. **FFI surface** (all in `crates/nmp-marmot/src/ffi.rs`):
   - `nmp_marmot_register(app, secret_key_hex, db_dir)` — explicit nsec
   - `nmp_marmot_register_active(app, db_dir)` — reads actor-owned key, no nsec exposed to Swift
   - `nmp_marmot_unregister(handle)` — teardown

3. **Write dispatch** (generic, not bespoke): `nmp_app_dispatch_action("nmp.marmot", action_json)` — the `MarmotActionModule` (`crates/nmp-marmot/src/projection/action.rs`) handles all mutating ops: `publish_key_package`, `create_group`, `invite`, `send`, `leave`, `remove`, `accept_welcome`, `decline_welcome`, `ingest_signed_event`, `clear_pending`. The legacy bespoke `nmp_marmot_dispatch` C symbol was deleted in PR 3 (2026-05-23).

4. **Projections** pushed on every snapshot tick:
   - `nmp.marmot.snapshot` (NMMS FlatBuffers sidecar + JSON generic)
   - `nmp.marmot.messages` (NMMG FlatBuffers sidecar + JSON generic)

5. **Inbound ingest** (fully closed): `MarmotIngestParser` registered under the `"marmot"` slot key for kinds `[443, 444, 445, 1059, 30443]` via `replace_kind_parser`. Added in PR #1145 (2026-06-12). Welcomes and messages received from relays surface in the next snapshot with no Swift involvement.

6. **iOS UI** (`ios/Chirp/Chirp/Features/`):
   - `MarmotGroupsView.swift` — group list, invites chip, create group sheet
   - `MarmotGroupChatView.swift` — message stream, composer, invite/leave toolbar
   - `MarmotInviteSheet.swift` — invite members to existing group
   - `NewGroupSheet.swift` — create group dialog

7. **`nmp-app-chirp` marmot feature**: activated by `--features marmot` in the iOS `justfile` build targets. `MarmotHandle` symbols flow into `libnmp_app_chirp.a` via rlib linkage (`apps/chirp/nmp-app-chirp/Cargo.toml`).

8. **Headless CI unblocked**: `NMP_MARMOT_MOCK_KEYRING=1` (shipped in PR #872, `crates/nmp-marmot/src/credential_store.rs`) bypasses the Apple Keychain on any platform. Prior blocker ("headless blocked by keychain entitlement") is resolved — the fix landed.

**Why SHOULD-WORK rather than WORKS**: No cross-client interop test was found running on CI against live devices. The in-crate round-trip (`crates/nmp-marmot/src/tests.rs: marmot_full_round_trip_create_giftwrap_join_message`) and `nmp-testing` exit-gate tests (`marmot_message_roundtrip.rs`, `marmot_forward_secrecy.rs`, `marmot_post_compromise.rs`, `marmot_key_package_lifecycle.rs`, `marmot_key_package_rotation.rs`) run in-process; `real_relay_marmot_roundtrip.rs` is `#[ignore]` (not in CI). No UI test exercises the Marmot flow on a live device.

---

### Android (Chirp): SHOULD-WORK-UNVERIFIED

**V-109 has been addressed — Android is now wired, not absent.**

Evidence:

1. **JNI layer** (`crates/nmp-android-ffi/src/marmot.rs`):
   - `Java_org_nmp_android_KernelBridge_nativeMarmotRegisterActive` — calls `nmp_marmot_register_active` through the Rust path (symbol retention for cdylib). Line 47.
   - `Java_org_nmp_android_KernelBridge_nativeMarmotUnregister` — calls `nmp_marmot_unregister`. Line 67.

2. **`marmot` feature** enabled in `android/app/build.gradle.kts` line 76: `"build", "--release", "--features", "marmot"`. This forwards through `nmp-android-ffi/marmot` → `nmp-app-chirp/marmot` → `nmp-marmot/ffi`, pulling all MLS symbols into `libnmp_android_ffi.so`.

3. **Kotlin bridge** (`android/app/src/main/java/org/nmp/android/KernelBridge.kt`):
   - `marmotRegisterActive(dbDir)` — line 277
   - `marmotUnregister()` — line 286
   - `dispatchAction("nmp.marmot", ...)` — shared generic path, no per-op symbols

4. **KernelModel Marmot ops** (`android/app/src/main/java/org/nmp/android/KernelModel.kt`):
   - `registerMarmotIfNeeded(dbDir)` — line 430
   - `createGroup`, `sendGroupMessage`, `publishKeyPackage`, `acceptWelcome`, `declineWelcome` — lines 444–467

5. **Android UI** (`android/app/src/main/java/org/nmp/android/ui/`):
   - `GroupsScreen.kt` — group list, key package row, pending invite rows, create group dialog, Marmot registration triggered on account change
   - `GroupChatScreen.kt` — message stream, compose send button

6. **Typed decoders**: `TypedMarmotDecoder.kt` decodes both NMMS (snapshot) and NMMG (messages) FlatBuffers sidecars. Android data model (`android/app/src/main/java/nmp/marmot/` — generated FlatBuffers Kotlin classes) is complete.

7. **Projections read** in `KernelUpdateFrameDecoder.kt` line 191-192: `marmotSnapshot` and `marmotMessages` decoded from `typedProjections`.

8. **Missing ops vs iOS**: Android `KernelModel` does NOT expose `leave`, `invite` (on existing group), or `remove`. The UI in `GroupChatScreen.kt` has no leave/invite/remove buttons. iOS has these in `MarmotGroupChatView`'s toolbar and `MarmotInviteSheet`. This is a feature completeness gap, not a structural blockage.

9. **Changelog** confirms: "Marmot/MLS Android (#888, V-109): MLS group messaging wired into the Android build via the existing C-ABI seam; Android gains a Groups tab backed by the Marmot push projection." V-109 is marked done, not open.

**Why SHOULD-WORK rather than WORKS**: Same reason as iOS — no integration test covers the Android end-to-end path. Also, leave/invite-existing/remove ops are unimplemented on the Android UI layer (though the `dispatchAction` seam supports them).

---

## Android Gap List (vs iOS feature parity)

| Gap | Location | What's Missing |
|-----|----------|----------------|
| No leave group UI | `GroupChatScreen.kt` (entirely absent) | Dispatch `{"op":"leave","group_id_hex":"..."}` |
| No invite-to-existing-group UI | `GroupChatScreen.kt` (no toolbar/sheet) | Dispatch `{"op":"invite",...}` |
| No remove member UI | No file | Dispatch `{"op":"remove",...}` |
| No pending-commit clear UI | No file | Dispatch `{"op":"clear_pending",...}` |

All four are functional at the Rust/dispatch_action level — Kotlin methods for these exist partially (`acceptWelcome`, `declineWelcome` are done but `leave`, `invite`, `remove` are not in `KernelModel`). File: `android/app/src/main/java/org/nmp/android/KernelModel.kt` lines 438–467 show the current set.

---

## Architectural Constraint Violations (logic in platform code)

### iOS violations — ADR-0032 compliant but worth flagging

The following display logic lives in Swift (`ios/Chirp/Chirp/Extensions/PubkeyFormatting.swift` and `MarmotGroupChatView.swift`), explicitly per ADR-0032:

- `shortHex` (hex abbreviation): `MarmotGroupChatView.swift:61,241` and `PubkeyFormatting.swift:24`
- `displayInitials` (avatar initials for sender pubkey): `MarmotGroupChatView.swift:234`
- `pubkeyColorHex` (avatar tint): `MarmotGroupChatView.swift:235`
- `relativeTimeFromUnixSeconds` (timestamp formatting): `MarmotGroupChatView.swift:245`
- Pluralisation: `MarmotGroupChatView.swift:186` — `liveGroup.memberCount == 1 ? "member" : "members"` 

These are explicitly sanctioned by ADR-0032 as presentation-layer formatting. The `MarmotMessageRow` struct carries raw hex pubkeys and Unix timestamps; Swift formats them. This is correct doctrine — not a violation.

**One genuine grey area**: `MarmotGroupChatView.swift:186` does a platform-side pluralisation (`liveGroup.memberCount == 1 ? "member" : "members"`), which is noted inline as ADR-0032 presentation layer. The `invites_chip_label` IS Rust-owned plural form. Member count plural is platform-owned. This is borderline but within the documented exception.

**No MLS protocol logic** was found in Swift or Kotlin. No event construction, no crypto, no ratchet state, no MLS group id construction outside of opaque hex strings.

### Android violations

`GroupsScreen.kt:377-378` has a `shortHex` helper identical to the iOS one:
```kotlin
internal fun shortHex(hex: String): String =
    if (hex.length >= 16) "${hex.take(8)}…${hex.takeLast(8)}" else hex
```
Used at line 246 for `welcome.inviterNpub` display. Equivalent to iOS `PubkeyFormatting.shortHex`. Under ADR-0032 this is presentation-layer formatting — permitted.

`GroupChatScreen.kt:132` calls `shortHex(message.senderPubkeyHex)` for the sender label. Same ruling — presentation formatting.

`KernelModel.kt:491-497`: `escapeJson()` is a string escaping utility for JSON string building used in manual dispatch JSON. This is boilerplate glue, not protocol logic, but it does construct the action JSON envelope in Kotlin rather than using a typed serializer. Not a D0 violation (no protocol semantics), but slightly fragile. iOS uses `JSONSerialization.data(withJSONObject:)` for the same purpose (`MarmotBridge.swift:447-450`). Minor quality issue, not a doctrine violation.

---

## FFI / Action-Verb Inventory

**C-ABI symbols (nmp-marmot):**
- `nmp_marmot_register(app, secret_key_hex, db_dir) -> *mut MarmotHandle` — `crates/nmp-marmot/src/ffi.rs:525`
- `nmp_marmot_register_active(app, db_dir) -> *mut MarmotHandle` — `crates/nmp-marmot/src/ffi.rs:549`
- `nmp_marmot_unregister(handle)` — `crates/nmp-marmot/src/ffi.rs:584`

**Deleted symbols** (no longer exist): `nmp_marmot_dispatch`, `nmp_marmot_snapshot`, `nmp_marmot_group_messages`, `nmp_marmot_string_free`, `nmp_marmot_fetch_key_packages`

**Chirp-owned identity helpers** (`apps/chirp/nmp-app-chirp/src/ffi/identity.rs`):
- `nmp_app_chirp_identity_restore(app, db_dir, test_nsec)` — restores persisted identity + registers Marmot
- `nmp_app_chirp_identity_sign_in_nsec(app, secret, db_dir)` — sign in + register Marmot
- `nmp_app_chirp_identity_remove_account(app, identity_id)` — remove identity

**Dispatch action namespace:** `"nmp.marmot"` — registered by `MarmotActionModule` (`projection/action.rs`)

**Op verbs:** `publish_key_package`, `create_group`, `invite`, `send`, `leave`, `remove`, `accept_welcome`, `decline_welcome`, `ingest_signed_event`, `clear_pending`

**Projection keys:**
- `nmp.marmot.snapshot` — file_identifier `NMMS`, schema_id `nmp.marmot.snapshot`
- `nmp.marmot.messages` — file_identifier `NMMG`, schema_id `nmp.marmot.messages`

---

## Keychain / Keyring Story

**iOS production**: `apple-native-keyring-store` (`AppleStore::new()`) on first success. Falls back to in-memory mock if Keychain entitlement is unavailable (`credential_store.rs:61-65`). When mock is used, `keyring_unavailable = true` is surfaced in the snapshot; the host sees the diagnostic but Marmot still initializes. **V-62 is fixed**: the old silent fallback (returning null vs silently using mock) is now a hard null return on unexpected Keychain failures.

**Android production (non-Apple target_os)**: `credential_store.rs:68-77` — always falls through to `install_mock_store()`, returning `Some(true)`. This means on Android, `keyring_unavailable` is ALWAYS `true` in production. The MLS SQLite DB key lives only in process memory — group secrets are lost on every app restart on Android. This is the **current Android limitation**: no persistent keyring backend for Android (no Android Keystore integration in `nmp-marmot`). Functional for testing/demo but groups become unjoinable on next launch.

**Headless CI escape hatch**: `NMP_MARMOT_MOCK_KEYRING=1` unconditionally installs the in-memory mock on any platform. This fix landed in PR #872 — the "headless blocked by keychain entitlement" issue from prior intel is resolved. The simulator can now run Marmot tests without Keychain entitlement.

---

## Test Coverage

**Rust in-crate tests** (`crates/nmp-marmot/src/tests.rs`): full round-trip — key-package publish, create group, NIP-59 gift-wrap welcome, join, post-join self-update, message send, message decrypt.

**nmp-testing exit-gate tests** (run with `--features` marmot):
- `marmot_key_package_lifecycle.rs` — key package publish/rotation
- `marmot_key_package_rotation.rs`
- `marmot_message_roundtrip.rs` — Alice sends, Bob decrypts
- `marmot_forward_secrecy.rs` — forward secrecy proof
- `marmot_post_compromise.rs` — post-compromise security
- `real_relay_marmot_roundtrip.rs` — `#[ignore]`, requires live relay (wss://relay.damus.io), NOT in CI

**No cross-platform tests**: no test exercises iOS Swift calling into Rust and Android Kotlin calling into the same Rust layer interoperably. No XCUITest covers Marmot end-to-end on a simulator. No Android instrumented test.
