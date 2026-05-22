# Opus direction review #77 — code-grounded critique (HEAD c0afb20a, 2026-05-23)

Five-question review. File:line citations only; nothing reused from BACKLOG.

---

## 1. Highest-risk thing in the codebase

**`nmp_app_create_new_account` is non-idempotent and the iOS Submit button has no debounce.** Two rapid taps mint two distinct keypairs.

- C-ABI entry: `crates/nmp-core/src/ffi/identity.rs:47-90` — sends `ActorCommand::CreateAccount` unconditionally. No inflight guard.
- Actor handler: `crates/nmp-core/src/actor/dispatch.rs:305-324` calls `commands::create_account`, which generates a fresh `Keys` every invocation.
- The `inflight_dispatches` dedup at `crates/nmp-core/src/ffi/action.rs:271` ONLY guards `nmp_app_dispatch_action`. The 36 direct `ActorCommand`-enqueuing C symbols (create_account, signin_nsec, signin_bunker, add_relay, switch_active, ...) bypass it entirely.
- Swift caller has zero protection: `ios/Chirp/Chirp/Features/OnboardingView+Components.swift:92` shows `model.createAccount(profile: profile)` with `.disabled(false)` (line further down) and an explicit comment "always enabled". 4 Hz emit + a stutter-tap during a slow first snapshot = two accounts, two nsecs, user can keep the second only.

This is a concrete production-incident vector: a user creates "their" account, loses keys to the duplicate, and the kind:0 they "published" was signed by an nsec the app no longer remembers. Severity exceeds the documented actor-panic risk (which has a visible red banner and `kernelIsDead` flag at `ios/Chirp/Chirp/Bridge/KernelModel.swift:68-79`).

**Fix shape:** generalise `inflight_dispatches` into a per-`ActorCommand`-key dedup at `app.send_cmd`, OR have `create_new_account` consult `IdentityRuntime` for an in-flight pending-account slot before issuing `ActorCommand::CreateAccount`.

## 2. Capability completely missing that blocks real users

**There is no kernel-driven write surface for "publish an arbitrary kind".** A second-app developer writing, say, a long-form NIP-23 client cannot dispatch a kind:30023 publish without either (a) registering a custom `ActionModule` that knows how to build the event, or (b) reaching for `ActorCommand::PublishUnsignedEventToRelays` — which is `pub(crate)` in `nmp-core` and unreachable.

Evidence: `nmp_app_dispatch_action` accepts only namespaces a host has registered. The 11 live registrations (`apps/chirp/nmp-app-chirp/src/ffi.rs:580-737` + `crates/nmp-nip29/src/register.rs:105-109`) are all bespoke shapes. There is no `nmp.publish_event` namespace that accepts `{kind, content, tags}`. The closest is `PublishAction::PublishNote` (kind:1 only) and `PublishProfile` (kind:0 only) at `crates/nmp-core/src/ffi/mod.rs:740-754`'s register seam → registered modules.

This is why `apps/fixture/nmp.toml` has `ios = false`: the fixture proves the desktop seam, but a second iOS app cannot publish anything the kernel doesn't already know how to model.

## 3. Delete immediately

**`ActorCommand::SignInBunker { uri }` (`crates/nmp-core/src/actor/mod.rs:170-172`).** The doc comment admits: "Transport is NOT yet wired (D0 forbids `nmp-core -> nmp-signers`); this validates the URI shape and surfaces a `last_error_toast`." The actual bunker handshake runs through `nmp-signer-broker` (`bunker_hook.rs`) which uses a completely different path. The `SignInBunker` arm is a 5-year-old dead surface that misleads readers into thinking nsec and bunker share a code path — they don't. Live bunker dispatch is `nostrConnectURI` → broker → `AddRemoteSigner`. The `SignInBunker` arm exists only to set an error toast.

Reading `apps/chirp/nmp-app-chirp/src/ffi.rs:112` (`nmp_app_chirp_register`) is similarly misleading next to `nmp_app_chirp_register_group_chat` / `_group_discovery` / `_dm_inbox` / `_follow_list` — four overlapping registration symbols at `apps/chirp/nmp-app-chirp/src/ffi.rs:260,327,357,397` that each take `*mut NmpApp` but cannot be expressed once. The split is artifactual; it makes the second-app FFI 5× larger than it needs to be.

## 4. Unenforced architectural invariant (the strongest finding)

**`schema_version` is contractually a refuse-decode signal but Swift never reads it.**

- Definition + contract: `crates/nmp-core/src/kernel/update.rs:11-16` — "If `schema_version` doesn't match the version the host was compiled against, the host should show an error and refuse to decode further — do not silently ignore unknown fields. A renamed or retyped field otherwise decodes to wrong/null data with no diagnostic signal."
- Constant: `crates/nmp-core/src/update_envelope.rs:42` (`SNAPSHOT_SCHEMA_VERSION: u32 = 1`).
- Swift consumer: `ios/Chirp/Chirp/Bridge/KernelBridge.swift` `decode(pointer:)` (lines 494-534) and the `KernelUpdate` struct (line 680). Neither reads, asserts, nor decodes `schemaVersion`. `grep -n schemaVersion ios/Chirp/Chirp/` returns zero hits.

A Rust schema bump that renames `metrics.actor_queue_depth` → `metrics.queue_depth` keeps the field optional and silently decodes `actor_queue_depth = nil`. The contract documented in update.rs is unenforceable until a host actually checks `update.schema_version != KERNEL_SCHEMA_VERSION` and panics or shows a fatal banner. This is precisely the "everyone assumes it's true but the type system doesn't enforce it" failure mode the question asks for.

**Fix shape:** add `let schemaVersion: UInt32` to Swift's `KernelUpdate` (non-optional, so a missing field is a decode failure); add a const Swift `KERNEL_SCHEMA_VERSION = 1` mirrored from Rust; in `decode()` assert equality before returning the result and propagate a fatal panic frame on mismatch.

## 5. What a second iOS app would actually look like

Concretely, today (write-side):

**Rust crate** (~400 LOC): an `nmp-app-<name>` staticlib that mirrors `apps/chirp/nmp-app-chirp/src/ffi.rs` line-for-line:
- `nmp_app_<name>_register(handle)` — calls `register_action::<MyAction>()` per typed `ActionModule` impl, plus `register_snapshot_projection(key, closure)` for whatever feed shape the UI reads.
- 4–8 register variants like Chirp's `_register_dm_inbox` / `_register_follow_list` (one per projection the UI consumes — see `apps/chirp/nmp-app-chirp/src/ffi.rs:112,260,327,357,397`).
- An `ActionModule` impl per write verb. For anything beyond kind:1, the dev has to wire a fresh `ActorCommand` variant in `crates/nmp-core/src/actor/mod.rs` AND its dispatch arm AND a per-NIP crate. There is no public "publish arbitrary kind" path (see #2).

**Swift bridge** (~2,500 LOC): hand-mirror these C symbols:
- The 54 generic `nmp_app_*` symbols (counted across `crates/nmp-core/src/ffi/`).
- The 8 `nmp_app_<name>_*` symbols above.
- The `KernelUpdate` Codable hierarchy at `ios/Chirp/Chirp/Bridge/KernelBridge.swift:680-` — 1,988 LOC of typed Decodable structs the developer must hand-author for every `projections["<key>"]` shape Rust emits. `nmp-codegen` exists (`crates/nmp-codegen/`) but emits nothing for Swift (BACKLOG F-05). This is the v1 wall, not nmp-wasm.

**What's missing for that wall to come down:** a `nmp-codegen` pilot that emits Swift Decodables for one projection (e.g. `timeline`). Until that exists, every per-app crate's Swift bridge is a hand-translation tax proportional to projection-shape count.

## Additional finding: Swift triple-parses every snapshot

`ios/Chirp/Chirp/Bridge/KernelBridge.swift:494-521` does three JSON passes per tick:
1. `JSONSerialization.jsonObject` on the outer `{"t":"snapshot","v":...}` envelope (line 498).
2. `JSONSerialization.data(withJSONObject: inner)` to re-materialise the inner payload as `Data` (line 514).
3. `JSONDecoder.decode(KernelUpdate.self, from: innerData)` (line 521).

Rust's emit path uses `serde_json::value::RawValue` (zero-copy outer wrap; `crates/nmp-core/src/actor/tick.rs:48`). The Swift side undoes that win. At 4 Hz × Chirp-typical 12 KB payload, that is ~150 KB/s of redundant parse work on the main display thread. Fix shape: parse the outer `{"t":...,"v":...}` envelope via a streaming decoder that hands the inner payload directly to `JSONDecoder`.
