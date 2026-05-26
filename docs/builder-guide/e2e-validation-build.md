# E2E Validation App — Build Instructions

Companion to [`e2e-validation-app.md`](./e2e-validation-app.md). This file is
the **historical how** for the deleted Pulse app: FFI extensions, two new Rust
integration pieces, Xcode project, lipo/xcframework, sim launch, iPhone install.
Pulse and Stress were deleted on 2026-05-18 and their validation goals were
merged into Chirp; use `ios/Chirp` for current iOS execution.

The builder is one agent in one session. Follow steps in order. Push per rung via `git push origin HEAD:master` (per `~/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/memory/agent-push-protocol.md`). Commit message prefix: `feat(pulse): L<N> …`.

---

## 1. The load-bearing builder work: extending the FFI surface

`crates/nmp-core/src/ffi.rs` currently exposes only timeline-reading commands (M1-era). Pulse needs sign-in, publish, account switching, and relay editing. **Without these, no SwiftUI screen can do anything.** This is the largest single chunk of new code.

For each new function:
1. Add a variant to `ActorCommand` in `crates/nmp-core/src/actor/mod.rs`.
2. Handle it in `crates/nmp-core/src/actor/` (split into a new `commands/` submodule if the actor file approaches 500 LOC — per AGENTS.md HARD-cap).
3. Add `extern "C" fn nmp_app_*` wrapper in `ffi.rs` (validate inputs as the existing wrappers do — `c_string_argument`, hex checks, etc.).
4. Append declaration to `ios/NmpPulse/NmpPulse/Bridge/NmpCore.h`.
5. Add a Rust unit test in `crates/nmp-core/src/actor/tests.rs` (or split file) exercising the new command path.

### 1.1 New FFI functions (signatures)

```c
// Constructor variant — REPLACES nmp_app_new for Pulse. nmp_app_new stays for
// NmpStress / harness compatibility (defaults to MemEventStore + tmp paths).
// Storage path is fixed at construction time: the kernel spawns the actor
// and constructs Kernel with the right store backend inside nmp_app_new_with_config.
void *nmp_app_new_with_config(const char *storage_abs_path);

// Identity / multi-session
void nmp_app_signin_nsec(void *app, const char *nsec_bech32_or_hex);
void nmp_app_signin_bunker(void *app, const char *bunker_uri);
void nmp_app_create_new_account(void *app);  // generates fresh keys, returns via state update
void nmp_app_switch_active(void *app, const char *identity_id);
void nmp_app_remove_account(void *app, const char *identity_id);

// Publishing (kernel resolves outbox automatically per D3)
//
// Optional-string convention (matches existing c_string_argument semantics in
// crates/nmp-core/src/ffi.rs): NULL pointer OR empty string OR whitespace-only
// is treated as "absent". The kernel's command handler decides the absent
// behavior: for reply_to here, absent = top-level note. A NEW helper
// `c_optional_string_argument` returns Option<String> rather than collapsing
// to None and dropping the call (which is what c_string_argument does for
// REQUIRED args). Builder adds this helper alongside the existing one.
void nmp_app_publish_note(void *app, const char *content, const char *reply_to_id_or_null);
void nmp_app_react(void *app, const char *target_event_id, const char *reaction);
void nmp_app_follow(void *app, const char *pubkey);
void nmp_app_unfollow(void *app, const char *pubkey);

// Relay management
void nmp_app_add_relay(void *app, const char *url, const char *role);  // role: "read"|"write"|"both"
void nmp_app_remove_relay(void *app, const char *url);

// Sync / sub control
void nmp_app_trigger_sync(void *app, const char *filter_json, const char *relay_url);
void nmp_app_open_timeline(void *app);  // opens FollowingTimeline for active account
```

### 1.2 AppState update extensions

The kernel's decoded snapshot/update shape gains these fields. Existing fields
stay intact. Historical raw-C builds emitted this through `update_callback` as
JSON; the canonical runtime update transport is FlatBuffers.

```jsonc
{
  "rev": 14721,
  "accounts": [
    {"id": "01HX…", "npub": "npub1…", "display_name": "alice", "signer_kind": "local"|"bunker"|"nip07", "status": "active"|"idle"}
  ],
  "active_account": "01HX…",
  "relays": [
    {"url": "wss://relay.damus.io", "role": "both", "status": "connecting"|"connected"|"auth-required"|"auth-ok"|"error", "events_in": 1240, "last_error": null}
  ],
  "publish_queue": [
    {"event_id": "abcd…", "kind": 1, "in_flight_relays": 2, "ok_relays": 0, "failed_relays": 0}
  ],
  "last_error_toast": null,
  "diagnostics": {
    "subs_active": 3,
    "snapshots_per_sec": 8,
    "nip77_bytes_in": 12480,
    "nip77_bytes_out": 4220
  },
  // existing timeline / profile / thread fields unchanged
}
```

### 1.3 Test-support FFI is unavailable in release builds

`nmp_app_inject_*` exist only under `#[cfg(feature = "test-support")]`. The simulator build is `--release` without that feature; the iPhone build is the same. The app must NEVER call those functions. Build doc §4 has the cargo flags.

---

## 2. The two new Rust integration pieces

### 2.1 `Nip65OutboxResolver`

**Location.** New module `crates/nmp-core/src/publish/nip65.rs`. Re-export from `crates/nmp-core/src/publish/mod.rs`. (Keep in `nmp-core` rather than a new `crates/nmp-nip65/` crate to avoid workspace churn this session; a future M2-tidy can extract it.)

**Signature.**
```rust
pub struct Nip65OutboxResolver {
    store: Arc<dyn EventStore>,
    /// READ-side discovery only (per D3). Used when resolving recipient
    /// read-relays for `#p`-tagged events and when no kind:10002 is known
    /// for an author whose events we want to FETCH. NEVER used to choose
    /// targets for a PUBLISH.
    read_discovery_fallback: Vec<RelayUrl>,
}

impl Nip65OutboxResolver {
    pub fn new(store: Arc<dyn EventStore>, read_discovery_fallback: Vec<RelayUrl>) -> Self;
}

impl OutboxResolver for Nip65OutboxResolver {
    fn resolve(&self, author: &str, p_tags: &[String], target: &PublishTarget) -> BTreeSet<RelayUrl> {
        // 1. If target == Explicit { relays }, return relays.
        // 2. Look up latest kind:10002 for `author` in store, parse write-relay tags.
        //    If empty AND target is a publish, return EMPTY (forces caller to
        //    surface a D6 toast "no write-relays declared" — D3 prohibits
        //    publishing to undeclared relays).
        // 3. For each p_tag (recipient inbox), look up latest kind:10002,
        //    parse read-relay tags. If empty, fall back to
        //    `read_discovery_fallback` (this is discovery, not write).
        // 4. Union (2) + (3). Return BTreeSet for determinism.
    }
}
```

**Unit tests (`crates/nmp-core/src/publish/tests.rs`).**
- `nip65_resolver_uses_author_writes_when_present`
- `nip65_resolver_returns_empty_for_publish_when_no_kind10002` (D3 — must NOT silently fall back)
- `nip65_resolver_uses_read_fallback_for_p_tag_recipients_only`
- `nip65_resolver_unions_recipient_reads_for_p_tags`
- `nip65_resolver_returns_explicit_unchanged`
- `nip65_resolver_handles_malformed_kind10002_gracefully` (logs + treats as empty)

**Publish-empty-targets handling.** When `resolve()` returns an empty set for `PublishTarget::Auto`, the `PublishEngine` MUST NOT silently swallow the publish. It MUST emit a `last_error_toast` describing the gap ("active account has no kind:10002 — go to Accounts → Relays → add a relay and publish a fresh kind:10002") and leave the event in the publish queue as `pending_relays_unknown` until the user resolves the gap.

**Wire.** Replace `StaticOutbox::default()` in `PublishEngine` construction with `Nip65OutboxResolver::new(kernel.store(), READ_DISCOVERY_FALLBACK)`. `READ_DISCOVERY_FALLBACK` is a `const &[&str]` in `publish/mod.rs`: `["wss://relay.damus.io", "wss://nos.lol"]` — used ONLY for recipient-inbox discovery.

**Budget.** ~150 LOC impl + ~150 LOC tests. Hard cap 300 LOC for the file.

### 2.2 `ActiveAccountReactor`

**Location.** NEW module `crates/nmp-signers/src/active_account_reactor.rs`. **Not in `nmp-core`** — `nmp-signers` already depends on `nmp-core`; placing the reactor in `nmp-core` would force a `nmp-core → nmp-signers` edge and create a cycle. The reactor is the integration glue that lives where the cycle is allowed.

The reactor talks to the kernel through a **core-neutral command surface** the kernel exposes: a new `pub fn submit_active_switch(from: Option<String>, to: Option<String>, signer: Option<AuthSignerFn>)` on the kernel's existing actor-command sender. The reactor builds those args from `nmp-signers` types; the kernel sees only `String` pubkeys + the existing `AuthSignerFn` callback (already defined in `crates/nmp-core/src/kernel/auth.rs` as `Arc<dyn Fn(&UnsignedEvent) -> Result<SignedEvent, String> + Send + Sync>`) — no `nmp-signers` import in `nmp-core`.

**Responsibility.** Subscribes to `AccountManager`'s `ActiveChangeObserver`. On `ActiveChangeEvent { previous, current }`:
1. Adapt the new active signer (`AccountManager::signer_active()`) into an `AuthSignerFn`.
2. Send ONE `ActorMsg::ActiveAccountSwitched { from, to, signer: AuthSignerFn }` to the kernel actor.
3. Actor handles the message atomically in one tick: closes account-A subs (kind:3 / kind:10000 / kind:10002 author=A + any `FollowingTimeline` rooted at A), rebinds `bind_auth_signer` to the new signer + pubkey, opens equivalent subs for account-B, and emits ONE snapshot after all rebuilds. The single-tick atomicity satisfies D4 (single writer per fact: the actor is the sole writer of subscription state) — D5 (snapshot boundedness) is a property of the snapshot itself, not the switch transaction.

**Signature.**
```rust
// In crates/nmp-signers/src/active_account_reactor.rs
pub struct ActiveAccountReactor {
    kernel_tx: Sender<ActorMsg>,
    manager: Arc<Mutex<AccountManager>>,
}

impl ActiveChangeObserver for ActiveAccountReactor {
    fn on_active_change(&self, ev: &ActiveChangeEvent) {
        // Resolve current signer via self.manager.lock().signer_for(ev.current.as_ref()).
        // Adapt to AuthSignerFn (closure capturing Arc<dyn Signer>).
        // Send ActorMsg::ActiveAccountSwitched { from: ev.previous.map(|id| id.to_string()),
        //                                        to:   ev.current.map(|id| id.to_string()),
        //                                        signer }.
    }
}
```

The new `ActorMsg::ActiveAccountSwitched { from: Option<String>, to: Option<String>, signer: Option<AuthSignerFn> }` lives in `crates/nmp-core/src/actor/mod.rs` next to the other `ActorMsg` variants. It carries only kernel-native types — no `nmp-signers` imports cross the boundary.

**Integration tests (`crates/nmp-testing/tests/active_account_reactor.rs`).**
- `switch_closes_old_subs_and_opens_new`
- `switch_rebinds_publish_signer`
- `switch_emits_single_full_state_snapshot`
- `add_then_switch_does_not_leak_subs` (run for 100 switches, assert subs count stable)
- `remove_active_clears_subs_and_signer`

**Budget.** ~200 LOC impl + ~200 LOC tests. Hard cap 300 LOC per file.

### 2.3 Why these two pieces are blocking

Without `Nip65OutboxResolver`, `nmp_app_publish_note` with `PublishTarget::Auto` returns empty relay set → publish silently goes nowhere → L2 demo fails.

Without `ActiveAccountReactor`, `nmp_app_switch_active` updates the AccountManager but the actor's subscriptions are stale → L3 demo shows the same timeline after switching.

Land both BEFORE the SwiftUI screens that depend on them.

---

## 3. Bridge layer — Path A (raw C FFI extension)

**Chosen path: A.** Path B (`nmp gen modules`) is the M14 UniFFI deliverable; doing it now blows scope.

Path A means: extend `ios/NmpStress/NmpStress/Bridge/NmpCore.h` shape into `ios/NmpPulse/NmpPulse/Bridge/NmpCore.h`. Swift uses the same `KernelBridge.swift` pattern (already in NmpStress) — `nmp_app_new()`, register callback that re-dispatches to main thread, decode `state` JSON into a `KernelModel` `@Observable` class.

Copy + adapt `ios/NmpStress/NmpStress/KernelBridge.swift` and `ios/NmpStress/NmpStress/KernelModel.swift` as starting points. Extend `KernelModel` with the new `accounts`, `relays`, `publish_queue`, etc. fields.

**M14 note.** This bridge is throwaway. UniFFI in M14 supersedes it. The app's hand-written Swift surface (screens + KernelModel structs) is what survives.

---

## 4. Xcode project scaffold

Use the same `xcodegen` pattern as NmpStress. Create `ios/NmpPulse/project.yml`:

```yaml
name: NmpPulse
options:
  bundleIdPrefix: com.example
  deploymentTarget:
    iOS: "17.0"
settings:
  base:
    SWIFT_VERSION: "6.0"
    IPHONEOS_DEPLOYMENT_TARGET: "17.0"
    GENERATE_INFOPLIST_FILE: YES
    CODE_SIGN_STYLE: Automatic
    DEVELOPMENT_TEAM: ""  # filled by user on first device build
targets:
  NmpPulse:
    type: application
    platform: iOS
    sources:
      - path: NmpPulse
        excludes:
          - Assets.xcassets
    info:
      path: NmpPulse/Info.plist
      properties:
        LSRequiresIPhoneOS: true
        UILaunchScreen: {}
        UIApplicationSupportsIndirectInputEvents: true
        NSAppTransportSecurity:
          NSAllowsArbitraryLoads: false
          NSExceptionDomains: {}  # all relays are wss:// so TLS, no exception needed
    settings:
      base:
        PRODUCT_BUNDLE_IDENTIFIER: com.example.NmpPulse
        PRODUCT_NAME: NmpPulse
        GENERATE_INFOPLIST_FILE: NO
        SWIFT_OBJC_BRIDGING_HEADER: NmpPulse/Bridge/NmpCore.h
        # SDK-conditional paths — Xcode picks per active SDK so device builds
        # never see the simulator arm64 archive (both are aarch64 but with
        # different ABIs; xcodebuild will silently link the wrong one if the
        # paths are unconditional).
        "LIBRARY_SEARCH_PATHS[sdk=iphoneos*]": "$(SRCROOT)/../../target/aarch64-apple-ios/release"
        "LIBRARY_SEARCH_PATHS[sdk=iphonesimulator*]": "$(SRCROOT)/../../target/aarch64-apple-ios-sim/release"
        OTHER_LDFLAGS: "$(inherited) -lnmp_core"
        ENABLE_USER_SCRIPT_SANDBOXING: NO
schemes:
  NmpPulse:
    build:
      targets:
        NmpPulse: all
```

Then: `cd ios/NmpPulse && xcodegen generate`.

**Note on `LIBRARY_SEARCH_PATHS`.** Per-SDK paths (above) ensure device builds link `aarch64-apple-ios/release/libnmp_core.a` and simulator builds link `aarch64-apple-ios-sim/release/libnmp_core.a` — they have the same triple-arch but distinct ABIs; without the SDK gate xcodebuild can pick the wrong one and you get cryptic link errors. For L5 (device), switch to the xcframework (§5.2) so there's a single artifact to vend.

**Files to create under `ios/NmpPulse/NmpPulse/`:**
```
NmpPulseApp.swift              # @main entry; instantiates KernelBridge.
Info.plist
Bridge/NmpCore.h               # C declarations (copy + extend from NmpStress).
Bridge/KernelBridge.swift      # FFI wrapper (adapt from NmpStress).
Bridge/KernelModel.swift       # @Observable state mirror; JSON decode.
Views/OnboardingView.swift     # Screen 1
Views/TimelineView.swift       # Screen 2
Views/NoteDetailView.swift     # Screen 3
Views/ComposeView.swift        # Screen 4
Views/AccountsView.swift       # Screen 5
Views/DiagnosticsOverlay.swift # gear-icon panel
Views/NoteRow.swift            # row component
```

Plus `Assets.xcassets/AppIcon.appiconset/` with placeholder icons (1024px PNG, sim is fine without; device install requires it).

---

## 5. Rust → iOS build

### 5.1 Per-rung build (fast iteration in simulator)

```bash
# From repo root. Simulator-only, release-mode (test-support feature OFF).
cargo build -p nmp-core --release --target aarch64-apple-ios-sim
ls target/aarch64-apple-ios-sim/release/libnmp_core.a  # confirm
cd ios/NmpPulse && xcodegen generate
```

Open `NmpPulse.xcodeproj`, pick a simulator destination, Run.

**If targeting an Intel-architecture Mac as a fallback:** also build `x86_64-apple-ios-sim` and create a fat library:
```bash
cargo build -p nmp-core --release --target x86_64-apple-ios-sim
lipo -create \
  target/aarch64-apple-ios-sim/release/libnmp_core.a \
  target/x86_64-apple-ios-sim/release/libnmp_core.a \
  -output target/libnmp_core-sim-fat.a
```

### 5.2 xcframework for device + sim (one artifact)

```bash
# Device (real iPhone)
cargo build -p nmp-core --release --target aarch64-apple-ios

# Simulator (Apple Silicon Mac)
cargo build -p nmp-core --release --target aarch64-apple-ios-sim

# Bundle into xcframework
rm -rf target/NmpCore.xcframework
xcodebuild -create-xcframework \
  -library target/aarch64-apple-ios/release/libnmp_core.a \
    -headers ios/NmpPulse/NmpPulse/Bridge \
  -library target/aarch64-apple-ios-sim/release/libnmp_core.a \
    -headers ios/NmpPulse/NmpPulse/Bridge \
  -output target/NmpCore.xcframework
```

Then update `project.yml` to depend on the xcframework rather than `LIBRARY_SEARCH_PATHS`. Defer this to L5 — for L1–L4 the bare staticlib + per-target search paths work and iterate faster.

### 5.3 Required iOS targets installed?

```bash
rustup target list --installed | grep -E "aarch64-apple-ios"
# Expected: aarch64-apple-ios + aarch64-apple-ios-sim
# Install if missing:
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
```

---

## 6. Test fixtures

Create `crates/nmp-testing/fixtures/test_nsec.txt` — a freshly-generated nsec (single line, no trailing newline), checked into git for repeatable demos. Builder generates via:

```bash
cargo run -q -p nmp-signers --example gen_nsec 2>/dev/null > crates/nmp-testing/fixtures/test_nsec.txt
```

(Add the example to `crates/nmp-signers/examples/gen_nsec.rs` — 10-line `Keys::generate().secret_key().to_bech32()` snippet.)

The app's "Paste nsec" path can be pre-populated with this in DEBUG builds for QA convenience. In RELEASE the field is empty.

---

## 7. Launching in iOS Simulator

```bash
# 1. Boot a simulator (any iPhone 15+ image)
xcrun simctl list devices available | grep "iPhone 15"
xcrun simctl boot "iPhone 15 Pro"
open -a Simulator

# 2. Build the app
cd ios/NmpPulse
xcodebuild -project NmpPulse.xcodeproj \
  -scheme NmpPulse \
  -destination "platform=iOS Simulator,name=iPhone 15 Pro" \
  -configuration Release \
  -derivedDataPath ./build \
  build

# 3. Install + launch
APP_PATH="./build/Build/Products/Release-iphonesimulator/NmpPulse.app"
xcrun simctl install booted "$APP_PATH"
xcrun simctl launch --console-pty booted com.example.NmpPulse

# 4. Logs
xcrun simctl spawn booted log stream --predicate 'subsystem == "com.nmp.pulse"' --level debug
```

Always use `-derivedDataPath` to keep build artifacts under `ios/NmpPulse/build/` rather than `~/Library/Developer/Xcode/DerivedData/` (disk-pressure mitigation per memory).

---

## 8. Installing on physical iPhone

**Prerequisites the builder asks the user to handle:**
1. Plug in iPhone via USB.
2. On the iPhone: Settings → General → VPN & Device Management → trust the developer profile after first install.
3. The user's Apple Developer team ID needs to be set. Builder prompts:
   > "Run `xcodebuild -showBuildSettings -project ios/NmpPulse/NmpPulse.xcodeproj | grep DEVELOPMENT_TEAM` and paste the team ID, or open the project in Xcode → Signing & Capabilities → select your team. CODE_SIGN_STYLE=Automatic so Xcode handles provisioning."

**Build + install:**
```bash
# Discover the device
xcrun devicectl list devices
# Note the device identifier (UUID-like)

cd ios/NmpPulse
# Prefer -destination "id=..." over "name=..." — name is ambiguous if multiple
# devices have the same name, and the UDID is exact.
xcodebuild -project NmpPulse.xcodeproj \
  -scheme NmpPulse \
  -destination "platform=iOS,id=$DEVICE_ID" \
  -configuration Release \
  -derivedDataPath ./build \
  -allowProvisioningUpdates \
  build

APP_PATH="./build/Build/Products/Release-iphoneos/NmpPulse.app"
xcrun devicectl device install app --device "$DEVICE_ID" "$APP_PATH"
# --console attaches stdout/stderr immediately (devicectl doesn't have a
# separate `console` subcommand — log streaming flows through `process launch
# --console`).
xcrun devicectl device process launch --console --device "$DEVICE_ID" com.example.NmpPulse
```

**Device-log capture (alternative if you want a structured filter):** use Console.app on the Mac with the device connected and the `subsystem == "com.nmp.pulse"` predicate, or `idevicesyslog` from `libimobiledevice` if Apple's tooling proves brittle.

If `devicectl` flow fails, fall back to Xcode GUI: open `NmpPulse.xcodeproj`, pick the physical device as destination, hit Run. Tell the user to "watch for the trust-dev-profile prompt on the device, accept it, re-run."

---

## 9. Verification checkpoints (per rung)

After each rung lands and pushes to master, run:

```bash
# Rust gates (always)
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings

# Real-relay smoke (after L2)
cargo test -p nmp-testing --features test-support \
  --test real_relay_smoke -- --ignored --nocapture

# iOS sim build (after each rung)
cd ios/NmpPulse && xcodebuild -scheme NmpPulse \
  -destination "platform=iOS Simulator,name=iPhone 15 Pro" \
  -configuration Release -derivedDataPath ./build build

# Manual demo (per §7 of app doc)
xcrun simctl install booted ./build/Build/Products/Release-iphonesimulator/NmpPulse.app
xcrun simctl launch booted com.example.NmpPulse
```

---

## 10. Per-rung commit plan

| L | Commits (push each via `git push origin HEAD:master`) |
|---|---|
| **Pre-L1** | `feat(ffi): extend ActorCommand + ffi.rs for sign-in / publish / multi-account / relay edit (8 new commands, NmpCore.h updated)` + `feat(publish): Nip65OutboxResolver wired into PublishEngine` + `feat(actor): ActiveAccountReactor closes-old-opens-new on switch (D5 atomicity)` |
| **L1** | `feat(pulse): L1 scaffold — Xcode project, KernelBridge, OnboardingView + TimelineView, nsec-paste sign-in, single-pubkey timeline reading from relay.damus.io in simulator` |
| **L2** | `feat(pulse): L2 — ComposeView + nmp_app_publish_note round-trip via Nip65OutboxResolver` |
| **L3** | `feat(pulse): L3 — AccountsView multi-session switcher + ActiveAccountReactor end-to-end` |
| **L4** | `feat(pulse): L4 — follow-edit publishes kind:3 (framework-magic C8/C13 rewires subs) + NoteDetail screen with replies + kind:7 reactions` |
| **L5** | `feat(pulse): L5 — xcframework + verified install on physical iPhone (L2 + L3 demos re-run on device)` |

Post-merge codex review after EACH push per memory protocol; FIX-IN-PLACE for typos/format.

---

## 11. Definitely-deferred (do NOT add scope)

- UniFFI / `nmp gen modules` codegen — M14 work.
- LMDB on iOS — if `cargo build --target aarch64-apple-ios` fails on the `heed` crate, drop to MemEventStore (the default). Document in commit message. M11 will revisit. The `nmp_app_new_with_config(storage_abs_path)` API still takes the path so the kernel can persist there once the LMDB backend is wired; until then the storage_abs_path is honored for the publish-queue durability file only.
- Bunker — if NIP-46 wiring in L3 takes more than 90 minutes, ship L3 with nsec-only multi-account and file a follow-up issue. (The two-account demo still passes with two nsecs.)
- Push notifications, NSE, deep-linking, App Store assets — productionization.

---

## 12. Hand-off contract

When the builder declares done, the deliverables are:
1. **Rust:** FFI surface extended, `Nip65OutboxResolver` + `ActiveAccountReactor` landed, `crates/nmp-testing/tests/real_relay_smoke.rs` checked in (with `#[ignore]` per test) — all `cargo test --workspace` green.
2. **iOS:** `ios/NmpPulse/` complete Xcode project, builds + runs in simulator, screens 1–5 functional at least at L4 fidelity.
3. **Docs:** brief `ios/NmpPulse/README.md` with 10-line "how to run" pointing back to this build doc.
4. **Evidence:** in commit messages of L1–L5, paste the time-to-first-row + publish-round-trip latency observed in the simulator.

QA agent (next dispatch) takes it from there.
