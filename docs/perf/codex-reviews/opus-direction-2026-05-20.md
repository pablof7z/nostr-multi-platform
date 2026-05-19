# NMP Directional Review — 2026-05-20

Reviewer: distributed-systems architect. Audience: lead developer. Verdict-first, opinionated.

NMP has a genuinely good kernel idea and is being undermined by scope sprawl and aspirational doctrine. The framework has ~17 nip crates, an MLS stack, a wallet, and a podcast app skeleton — but the production write path is still in-memory and no automated test crosses the FFI boundary. You are decorating a house with no foundation poured. The work below is about pouring the foundation.

## 1. Kernel actor model and snapshot delivery

**Right.** Single-writer actor (D4) is the correct spine. TEA on one thread, JSON snapshots ≤60Hz, intents back — this is the right shape and you should not second-guess it. `changed_since_emit()` idle gating (D8) is exactly the discipline most Nostr clients lack.

**Wrong.** The actor blocks up to 5s on NIP-46 bunker round-trips. A single-writer actor that can block is a single-writer actor that freezes the whole app. This is not a "later" item — it is a correctness defect in the spine.

**Missing.** Snapshot schema versioning. A full-JSON snapshot with no version tag means any field change silently desyncs an old shell.

**Recommendation.** Make the actor non-blocking for *all* signer ops now: dispatch SignerOp, return `Pending`, resume on the response message. Treat bunker signing as just another async fact arriving on the actor mailbox — same pattern as a relay event. Add a `schema_version` integer to every snapshot envelope.

## 2. Extension/module system

**Wrong, and this is the clearest call in the review.** `ModuleRegistry` in `crates/nmp-core/src/substrate/mod.rs` stores `Vec<ModuleDescriptor>` — `namespace`, `family`, `rust_type` strings. No closures, no dispatch. The five traits (ViewModule, ActionModule, DomainModule, CapabilityModule, IdentityModule) compile but the kernel never calls through them. This is documentation theater. Apps actually wire behavior through `KernelEventObserver` — a flat raw-event fan-out — which is a *different, simpler, working* mechanism.

**Recommendation.** Pick one. Either (a) delete the five-trait substrate and keep `KernelEventObserver` as the v1 extension story, documenting it honestly as "flat event fan-out, host composes projections" — or (b) commit to building the dispatch runtime. Do **(a)**. The observer model is sufficient for Chirp and any v2 app; the trait system is a research project masquerading as shipped code. Dead scaffolding costs every reader of `nmp-core` comprehension tax and lies in the spec. Delete it this week.

## 3. FFI surface and codegen gap

**Right.** D6 (no exceptions across FFI) is enforced — `catch_unwind`, null-degrades-silently. The ~2,400 LOC FFI surface is disciplined.

**Wrong.** `nmp-codegen` exists (~556 LOC, `ffi_gen.rs` / `generate.rs` / `manifest.rs`) but `justfile` only points it at `apps/fixture/nmp-app-fixture`. **Chirp does not consume codegen** — its bindings are hand-written. So the one real app validates nothing about the generated path, and the fixture app validates nothing about a real app. The codegen is being built against a strawman.

**Missing.** A generated, versioned schema contract that Chirp actually links.

**Recommendation.** Point codegen at Chirp. If codegen cannot yet express what Chirp needs, that gap *is* the codegen backlog — discover it against the real app, not the fixture. Until Chirp consumes generated bindings, treat codegen as unproven.

## 4. Protocol module design

**Wrong — crate proliferation.** The task brief listed ~10 nip crates; the tree has ~17 (nip01/22/23/29/42/51/57/59/77, plus nmp-reactions, nmp-content, nmp-threading, nmp-nwc...) and an `apps/podcast` skeleton that MEMORY says was explicitly killed. Each crate is a Cargo edge, a CI target, a version surface. Most are thin. You are paying microservice tax inside a monorepo.

**Wrong — NWC is a D0 violation.** `WalletStatus` is a field in the kernel snapshot (`crates/nmp-core/src/kernel/mod.rs:290`) and `nmp-core` depends on `nmp-nwc`. D0 says no app nouns in the kernel. A wallet is an app noun. Either D0 is real or it isn't.

**Recommendation.** Hold the line on D0: NWC moves out of `nmp-core` and becomes a module wired via `KernelEventObserver`, exactly like Chirp's timeline. If that is too painful, you have learned D0 is not actually your doctrine — then rewrite D0 honestly rather than keeping a rule the code violates. For nip crates: collapse the thin ones. A crate should exist when it has an independent consumer or a real compile-isolation reason; otherwise it is a module.

## 5. Relay pool strategy

**Wrong — undeclared fork.** `relay_worker/mod.rs` + `relay.rs` ≈ 740 LOC of hand-rolled reconnect/auth/pool logic duplicating `nostr-sdk`. Burden of proof is on the fork: you must be able to *name* what nostr-sdk's pool cannot do. "We wanted control" is not a reason; reconnect/backoff/NIP-42 auth are exactly the boring, bug-prone code you want upstream maintaining.

**Recommendation.** Write a one-page ADR listing concrete capabilities nostr-sdk's relay pool lacks (negentropy hooks? per-relay role routing? deterministic test injection?). If the list is short, adopt nostr-sdk and delete the fork. If genuinely long, keep the fork but *declare* it in an ADR so it stops being silent debt. My bet: the list is short.

## 6. Testing strategy and quality gates

**Wrong — the highest-risk gap.** CI runs `cargo test` in Rust isolation. **Nothing automated drives C FFI → Swift.** The contract that the entire architecture rests on — kernel emits JSON, shell decodes it — is validated only by a human running the app. Every snapshot schema change is an unguarded change.

D2 is also aspirational: `CompiledPlan` permits REQ without a negentropy gate. A doctrine the planner does not enforce is a comment.

**Recommendation.** Build an FFI E2E harness: a Rust or Swift `XCTest` target that loads the dylib, registers an observer, feeds canned events, and asserts on decoded snapshots. This is days of work and it is the cheapest insurance you will ever buy. Then make D2 a compile/runtime gate inside `CompiledPlan` — REQ construction should be unrepresentable until negentropy is checked.

## 7. The "thin shell" proof — is Chirp doing it right?

**No.** Two hard data points. (a) The shell is **7,928 Swift LOC** against a 2,439-LOC FFI surface. A thin shell does not outweigh its kernel interface 3:1. (b) `ios/Chirp/Chirp/Components/NoteContentView.swift` runs a regex `/nostr:[a-z0-9]+|https?.../ ` over note content and branches on `hasPrefix("npub1")` — that is **protocol-level content parsing in Swift**. The kernel should emit pre-segmented content runs; the shell should only render them. Right now the doctrine has already lost in the one app meant to prove it.

`nmp-app-chirp` is 1,445 LOC (not the 800 the brief claimed) and carries policy — keychain fallback, gift-wrap subscription policy. Some glue is fine; *policy* in glue is leakage.

**Recommendation.** Move content segmentation into `nmp-nip01` so the kernel emits typed runs (`text` / `mention(pubkey)` / `link` / `hashtag`). Re-measure Swift LOC after — that delta is your leakage scorecard. Pull policy decisions out of `nmp-app-chirp` into kernel capabilities (D7: capabilities report, kernel decides).

## 8. v1 vs post-v1

**Ship in v1:** non-blocking signer; real LMDB write path wired (it exists, it is just not the production store — finish it); negentropy gate enforced; FFI E2E harness; content segmentation in the kernel; `KernelEventObserver` as the *only* documented extension story.

**Defer post-v1:** Marmot/MLS (shipping encrypted groups before the write path is hardened is a textbook priority inversion — MLS group state on an in-memory store is data loss waiting to happen), multi-account atomic switch, Android/web shells, codegen-for-Chirp, the five-trait substrate (or delete it).

---

## Action List — highest leverage first

1. **Wire the LMDB write path as the production store.** *Why:* every other feature writes data; an in-memory store means cold-launch data loss and makes MLS unsafe. This is the foundation. *Effort:* medium — the crate and trait exist; finish the write path and flip the default backend.

2. **Build the FFI → Swift E2E test harness.** *Why:* the architecture's central contract is currently human-validated only; every schema change is unguarded. *Effort:* low-medium — one XCTest target loading the dylib with canned events.

3. **Make the actor non-blocking for all signer ops.** *Why:* a single-writer actor that blocks 5s freezes the whole app; this is a spine-level correctness bug. *Effort:* medium — model SignerOp responses as actor mailbox messages.

4. **Delete the dead five-trait substrate; bless `KernelEventObserver` as the v1 extension story.** *Why:* `ModuleRegistry` stores only descriptors — it dispatches nothing; the scaffolding lies to every reader and to the spec. *Effort:* low — deletion plus a doc update.

5. **Enforce D2: no REQ without a passing negentropy gate in `CompiledPlan`.** *Why:* a doctrine the planner does not enforce is a comment, not a guarantee. *Effort:* low-medium — make the un-gated REQ unrepresentable in the type.

6. **Move NWC out of `nmp-core`; move content segmentation into `nmp-nip01`.** *Why:* `WalletStatus` in the kernel snapshot violates D0; Swift parsing `nostr:` URIs violates the thin-shell premise. Both restore the doctrine the project claims. *Effort:* medium.

7. **Write the relay-pool ADR — adopt nostr-sdk unless the fork is justified in writing.** *Why:* ~740 LOC of duplicated reconnect/auth is silent debt; force the decision into the open. *Effort:* low to decide, medium if you adopt.

Items 1–3 unblock v1 shipping. 4–7 pay down the doctrine-vs-reality gap that will otherwise rot the framework's credibility.
