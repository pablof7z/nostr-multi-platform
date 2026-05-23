---
title: "Opus direction review #13 — adoption surface: the gaps outside the kernel"
date: 2026-05-24
author: Opus (senior distributed-systems architect, code-grounded)
scope: Direction review — what NMP should support, stop doing, and substantially improve. Focused on angles outside the kernel/FFI boundary that previous reviews stayed inside of.
verified-against: HEAD on master (worktree agent-a04f07d7b706ceb94; files cited individually).
prior-reviews-built-on: #8 (HostOpHandler), #10 (ActorCommand god enum), #11 (DX after Notes spike), #12 (abstraction leverage — dispatch_action/capability split, ActionModule adoption).
do-not-re-litigate: F-05 codegen sweep · 48-symbol nmp_app_* deprecation calendar · MlsOpHandler→HostOpHandler rename · V-04 dual-subscription removal · ActorCommand god enum split · KernelBridge.swift 1,892 LOC.
---

# Headline

Reviews #6–#12 audited the kernel/FFI seam exhaustively and found it sound but cluttered. They missed the bigger gap: NMP's *adoption surface* — the CLI, the test library, the cross-platform shells, the public distribution — has not caught up to the kernel's maturity. The aim.md promise ("one-shot a working Nostr application … all four platforms") is structurally false today not because the kernel can't host it, but because nothing outside `crates/nmp-core/` makes it reachable to a developer who is not already inside this monorepo. The 90-day decision below is a fork in identity, not a feature picklist.

---

# Findings

## 1. **Fix** — `nmp init` is monorepo-only; the scaffold cannot live outside this checkout

`crates/nmp-cli/src/init.rs:74,142` resolves `nmp-core` to **the absolute path of *this* checkout** (`fs::canonicalize(crates/nmp-core)`) and bakes it into the generated `Cargo.toml`. `crates/nmp-cli/templates/README.md.tmpl:31-34` makes this explicit: *"place this app inside an `nmp` checkout (so `crates/nmp-core` resolves) and add `apps/{name}/nmp-app-{name}` to the `[workspace] members` list."* No crates.io publication exists. **Impact:** the headline DX claim from aim.md §1 is unreachable to anyone outside this repo — the scaffold is a monorepo demo, not a CLI.
**Next action:** publish `nmp-core` and the protocol crates to crates.io behind a `0.1.0-alpha` tag; rewrite `init.rs:74-89` to emit a crates.io-versioned dep with a `--local-nmp <path>` flag for monorepo work. Until this lands, every CLI demo lies.

## 2. **Fix** — `nmp init` scaffolds a Rust skeleton, no iOS / Android / wasm

The scaffold emits four files under `crates/<name>-core/` plus a `Cargo.toml`, `nmp.toml`, `README.md` (`init.rs:84-93`). It does **not** emit any `ios/`, `android/`, `web/`, or `desktop/` shell. `apps/notes/README.md:69-105` is the proof: the best-known second app required hand-creating an Xcode project, hand-copying `NmpCore.h` from Chirp, hand-configuring `SWIFT_OBJC_BRIDGING_HEADER`, hand-writing a 5-step build phase. The aim.md "ship with sane defaults on all four platforms without ever touching relay routing" claim collapses on step 1: the developer is still inside Xcode Build Settings, not relay routing.
**Next action:** add `nmp init --shell ios` (later `--shell wasm`) that scaffolds the four artifacts Notes wrote by hand: `project.yml` (xcodegen), an `ios/<App>/Bridge/NmpCore.h` symlink, an `.entitlements`, and a `cargo build` pre-build phase. `apps/notes/` is the working template — copy from there.

## 3. **Extend** — `nmp-testing` is a workspace-internal LMDB harness, not a public testing library

`crates/nmp-testing/src/lib.rs:6` exports a single module: `store_harness` (277 LOC), purpose-built for `EventStore` backend tests. `MockRelay`/`LocalRelay` are used in seven `crates/nmp-core/src/**` test files but are **not re-exported** from `nmp-testing`. A developer following the Notes template who wants to test their `ActionModule` against a mock relay has nothing to import. **Impact:** the framework promise extends to "write your app on the kernel" but stops at "test it" — every new app reinvents the fixture builder, and untested apps ship broken.
**Next action:** create `nmp-testing/src/{relay,signer,event_factory,actor_probe}.rs` that re-export and wrap the kernel-internal helpers. Document the four-line "spin up an actor against a MockRelay" pattern in `docs/testing.md`.

## 4. **Fix** — "four-platform" claim is honest only for iOS

Android Chirp is **259 LOC total** across `KernelBridge.kt:56`, `KernelModel.kt:133`, `MainActivity.kt:70`. Desktop is a **545 LOC egui demo** (`crates/nmp-desktop/src/{app,bridge,main,render,snapshot}.rs`). Wasm Stage 3c just landed (PR #385) but IndexedDB persistence is still v1-blocking (F-01). `justfile` has exactly one platform runner recipe: `rust-ios-sim`/`run-ios`. There is **no** `run-android`, `run-desktop`, `run-wasm`. Plan.md v1 exit criterion #6 already flags this; the text understates because Android is also not at parity.
**Next action:** make a binary call by end of 90 days — fund Android Chirp to a parity matrix (compose/read/DM/wallet) **or** rewrite aim.md §1 to "iOS + macOS + web." The current state is the worst of both: enough Android/desktop code to look like a claim, not enough to back it.

## 5. **Keep, but reframe** — Marmot is not a removable distraction; it *is* the substrate's hardest test

`crates/nmp-core/src/ffi/mod.rs:291,329,388` reserves three slots specifically for MLS: `mls_local_nsec`, `pending_mls_autopublish` (`AtomicBool`), `mls_op_handler`. `crates/nmp-core/src/actor/dispatch.rs:862` contains the `ActorCommand::DispatchMlsOp` arm. ADR-0025 carves it out as an exception. That carve-out is the load-bearing evidence: **if the substrate needs a one-protocol exception, the substrate failed for that protocol's class.** MLS post-compromise / forward-secrecy state lives in `nmp-app-marmot` (~6.1k LOC across 23 files); generic event-store + projection semantics cannot host it without leaks. nmp-nip29 (the MLS-adjacent generic group infra) is consumed by both `nmp-app-marmot` and `nmp-app-chirp` (`grep -rln nmp-nip29 apps/`) — it is load-bearing, not removable.
**Next action:** promote the MLS-shaped exception to a documented `StatefulCryptoModule` extension seam in `docs/aim.md` §4. Stop calling it "exception"; call it "the second substrate tier for protocols that own their own forward-secret state." Then rename `mls_local_nsec`/`pending_mls_autopublish`/`DispatchMlsOp` to `crypto_module_local_nsec`/`pending_crypto_autopublish`/`DispatchCryptoOp` — same shape, name no longer leaks one specific protocol into core.

## 6. **Remove** — drop the implicit "build a Rust NDK / Applesauce" framing

The aim.md §4 synthesis-of-NDK-and-Applesauce framing made sense at project genesis. Today, Nostr SDK adoption is dominated by **language runtime**, not API quality: NDK wins JS apps; nostr-sdk wins server/CLI. NMP's actual moat is **iOS-class native + actor-correctness guarantees** — things RxJS/Tokio can't structurally enforce. **Impact:** "be Rust NDK" pushes roadmap toward parity features (Blossom, WoT, full wallet — `grep -rn "blossom\|web.*of.*trust"` in `crates/` returns **zero**) and away from the structural-correctness moat.
**Next action:** rewrite aim.md §4's "synthesis" paragraph to: *"NMP is the framework for native Nostr apps where correctness under multi-account / multi-relay / multi-signer concurrency is the product."* Filter the post-v1 list ruthlessly — anything that does not pay rent against "impossible-to-break-by-construction" gets cut, including Blossom and WoT.

## 7. **Fix** — Notes spike has nothing to copy from for tests, build, or distribution

`apps/notes/` proves the **runtime** thesis (PR #377): 25 LOC Rust + 96 LOC Swift bridge + zero new bespoke FFI symbols. But it ships **no tests**, no `xcodegen` `project.yml`, no CI integration. The bridge file `apps/notes/ios/Notes/Bridge/NmpCore.h` is a **verbatim 448-LOC copy** of `ios/Chirp/Chirp/Bridge/NmpCore.h` — a fork that silently diverges the moment Chirp adds an FFI symbol. **Impact:** the spike proved the framework can host a second app at write time, but a developer following the README ends up with two copies of NmpCore.h that drift.
**Next action:** make `NmpCore.h` a **build output** of `nmp-codegen` (it already emits Swift `Decodable`s — emit the header from the same source-of-truth pass) and have `nmp init --shell ios` symlink to that artifact instead of copying. Close the divergence window before it opens.

## 8. **Extend** — NIP-17 substrate exists but no app can light up DMs end-to-end without bespoke wiring

`crates/nmp-nip17/` is ~2k LOC of substrate across `display.rs`, `action.rs`, `dm_relay_list.rs`, `inbox.rs`, `dm_runtime.rs`. NIP-04/44/59 primitives are correctly delegated to the `nostr` crate. F-02 (DM cold-start receive verification) is still a v1 blocker per plan.md TL;DR; V-08 documents bunker users see an empty DM inbox with no signal. **Impact:** "DMs out of the box" is a top-three aim.md sample feature; today an app that wants DMs needs to manually wire NIP-17 substrate plus add Swift banners for the bunker-empty case. Notes pointedly does *not* implement DMs — doing so would have broken the "zero new C-ABI symbols" claim.
**Next action:** ship a `DmModule` in `nmp-nip17/src/lib.rs` that exposes a `Dm` `ActionModule` (`Send`, `MarkRead`) + generic projection keys (`dm.conversations`, `dm.thread:<peer>`). Then re-run the Notes-pattern spike for a DM app under the ≤300 LOC Swift budget to prove it.

---

# The single most important strategic decision NMP must make in the next 90 days

**Pick one identity: NMP is either a *framework* (publish to crates.io; `nmp init` produces a runnable iOS + Android + wasm app; `nmp-testing` is a real public library; Android reaches Chirp parity) or it is the *Chirp engine* (delete `crates/nmp-cli/`, delete `apps/{notes,longform}/`, retire the aim.md four-platform claim, ship iOS hard). The current roadmap implicitly attempts both — that is why the kernel is 136k LOC at the same time `nmp init` emits a monorepo-bound Rust skeleton and Android Chirp is 259 LOC; you cannot finish either path without giving up the other.**
