# Pending User Decisions

Decisions I made autonomously while the user was asleep, with my reasoning. If the user disagrees with any, the noted commit can be reverted or amended.

Format: one entry per decision. Surface every entry in every status update until the user explicitly acknowledges or supersedes.

---

## Open (need user review)

### PD-005 — T59: iOS signer binding for NIP-42 (deferred from T58)

**Decision (autonomous, 2026-05-18, T58):** T58 shipped the kernel-side NIP-42 wiring (parsers + per-relay driver + AuthGate routing + 5 spec'd integration tests + 1 bonus regression). iOS signer binding is deferred to a follow-up task T59.

**Why deferred:**

T58's task description named "iOS signer binding verification if straightforward, else file T59." It is not straightforward — it requires:

1. New FFI symbol `nmp_app_bind_auth_signer(NmpApp*, const char* pubkey_hex, signer_callback*, void* ctx)` in `crates/nmp-core/src/ffi.rs`.
2. New `ActorCommand::BindAuthSigner { pubkey_hex, signer: AuthSignerFn }` variant in `crates/nmp-core/src/actor/mod.rs` + dispatcher.
3. Adapter trampoline that wraps the C callback into an `AuthSignerFn = Arc<dyn Fn(&UnsignedEvent) -> Result<SignedEvent, String> + Send + Sync>`.
4. Swift side in `ios/NmpStress/NmpStress/KernelBridge.swift` that holds the `nmp_signers::AccountManager` (via `NmpSigners` UniFFI bindings — themselves M14 territory and not yet wired) and exposes a C-compatible callback.
5. UniFFI scaffolding for the signer slice OR a hand-rolled C shim — both are M10.5 / M14 surface decisions.

Conservative estimate: 250-400 LOC across 5+ files, 2 crates, plus Swift work, plus the M10.5 FFI hardening review since this adds a new FFI ingress for a closure pointer (D6 / errors-never-cross-FFI applies).

T58's 500 LOC hard cap could not absorb this without skimping on the integration tests; the cleaner separation is to land T58 as a kernel-substrate commit and file T59 for the FFI/iOS wiring.

**What T58 ships that makes T59 mechanical:**

- `Kernel::bind_auth_signer(pubkey_hex: String, signer: AuthSignerFn)` — already in place, callable from the actor as soon as an `ActorCommand::BindAuthSigner` is added.
- The signer callback shape (`Fn(&UnsignedEvent) -> Result<SignedEvent, String>`) is intentionally narrow so adapters from `nmp_signers::Signer` trait, the publish engine's `Signer::sign_auth` shim, or a hand-rolled C closure all fit without cycles.
- The kernel's NIP-42 handshake runs synchronously from `handle_text`. Async signers (NIP-46 bunker via `SignerOp::Pending`) need an extension — likely a future `deliver_signed_for(challenge, result)` API on the kernel mirroring `nmp_nip42::Nip42Driver::deliver_signed_for`. T59 should decide whether to inline that or expose the async hook at FFI boundary.

**Validation gap T59 closes:**

T58's tests verify the kernel handshake in isolation (synthetic signer closure). They do NOT verify that:
- The Swift-side AccountManager wiring is correct (signer slot is actually populated at iOS app startup).
- Real `LocalKeySigner::sign(unsigned)` produces a SignedEvent the kernel correctly forwards.
- A NIP-46 bunker round-trip works end-to-end on iOS.
- The signer is correctly **un**-bound on `AccountManager::remove(active_id)` (the `ActiveChangeEvent { current: None }` path) so a logged-out user cannot accidentally sign an AUTH event.

The first three are M14 (UniFFI) + M10.5 (live iOS demo) overlap. The fourth is a discrete correctness item — T59 should pin it with a regression test once the binding is in place.

**Recommendation:** dispatch T59 after M14's UniFFI scaffolding lands (so the Swift side has a real `NmpSigners` API to bind from). Tracking task: T59. If you want it sooner, the hand-rolled C shim path is feasible in isolation but creates a divergence with M14's planned UniFFI surface.

**If wrong:** revert T58's `bind_auth_signer` field on Kernel + the ActorCommand if added in T59; the signer plumbing is opt-in and the AUTH handshake degrades gracefully to "stay in ChallengeReceived" when no signer is bound (the bonus regression test `nip42_kernel_auth_without_signer_holds_in_challenge_received` pins this).

---

### PD-004 — M6 `IdentityId = pubkey_hex` vs ULID for "same nsec, two accounts"

**Decision (autonomous, 2026-05-18, T43):** keep `IdentityId = pubkey_hex` for the M6 landing.  ULID-based account ids are required before M8 (multi-account UX) ships, per `docs/research/sessions/synthesis.md` §1.2 (applesauce allows two accounts for the same pubkey — "same nsec, different relay policy" or "same bunker user from two devices").

**Why now:**

- The M6 demo is single-active-account (paste nsec / paste bunker / generate, then compose).  One-account-per-pubkey is fine for the demo.
- Switching to ULID is a 30-line change confined to `crates/nmp-signers/src/identity/manager.rs` — keying the `accounts` HashMap by ULID instead of `pubkey_hex` plus storing `pubkey` as a field on a small `AccountSlot { id: ULID, pubkey, signer }` record.
- `IdentityId` is a type alias for `String` today; the API surface does not change.
- Doing the switch before any UX flow lands keeps the eventual migration trivial.

**Recommendation:** ULID-rekey before M8 dispatch (filed as a sub-task on the M6 follow-up checklist in ADR-0015).  No user input needed unless you'd rather defer past M8.

---

### PD-003 — M7 publishing-pipeline scope (task #45) shipped as substrate-only ahead of M3/M6/M8 wiring

**Decision (autonomous):** shipped `crates/nmp-core/src/publish/` with engine + state machine + trait shims + 20 tests. Did NOT wire it into the actor / FFI / iOS slice. Did NOT use MockRelay (does not exist). Did NOT exercise real LMDB persistence.

**Background:** task #45 spec asked for a fully-wired publishing pipeline with NIP-65 outbox routing, AUTH-REQUIRED reauth via real signer, durable LMDB queue, MockRelay integration tests in `crates/nmp-testing/tests/`, etc. Three dependencies named in the task — #43 (M6 Signer), #46 (M8 RelayManager) — and one implicit dep (M3 LMDB store for publish queue rows) are all either not landed or only partially landed for adjacent concerns (M3 store covers events, not publish queue).

The task's own escape clause: "If one missing: define minimal trait shim that #43/#46 will satisfy when they land." I extended that clause to cover all three.

**What shipped:**
- `PublishEngine` with deterministic per-(event,relay) state machine: Pending → InFlight → Ok | RelayError | TimedOut → FailedAfterRetries
- Retry policy: AUTH-REQUIRED → reauth +1 retry; transient → up to 3 total attempts (initial + 2 retries) at 1s/4s
- `PublishStatusView` with bounded snapshot (rev counter, in_flight, recent_ok cap 32, recent_errors cap 32)
- Traits: `Signer`, `RelayDispatcher`, `OutboxResolver`, `PublishStore` — each with in-memory/noop/static test impl
- 11 unit tests (state machine + engine), 9 integration tests (NIP-65 routing, retry, give-up, restart, dedup, outcome classification)
- `docs/plan/m7-publishing.md` capturing scope + wiring deferred to dependency milestones

**Known weaknesses surfaced for codex/user review:**
- `publish_durable_across_restart` shares one `Arc<InMemoryPublishStore>` across the two engine instances — that's two engines reading the same in-process `Mutex<HashMap>`, not a serialize/deserialize round-trip through actual storage. The proof is weaker than the test name implies; the M3 LMDB-backed `PublishStore` impl will need its own round-trip test to close this gap.
- `PublishModule::reduce` (the ActionModule impl) is a syntactic pass-through. Real orchestration goes through `PublishEngine` direct methods. M6 ledger bridge will translate `ActionInput::RelayOk` → `PublishEngine::on_ack`.
- Engine consumes `Arc<dyn Signer>` for AUTH-REQUIRED retries but `apply_verdict::Reauth` currently models reauth as a transient backoff retry (no actual `sign_auth` call). M6 plumb-through will close this by calling `signer.sign_auth` between the verdict and the retry dispatch.
- File `crates/nmp-core/src/publish/tests.rs` (338 LOC) and `crates/nmp-core/tests/publish_engine.rs` (390 LOC) exceed the 300 LOC soft cap. Both under 500 hard cap. Precedent: `crates/nmp-testing/tests/m2_subscription_compilation_audit.rs` (460 LOC). Did NOT split.

**Hard-reset orphan commits:** during rebase I hard-reset to `origin/master` to escape doc-only conflicts in `docs/design/framework-magic/`. Approximately 7 doc-edit commits previously on `origin/worktree-agent-a53de6ee35b4e2ccc` (T22 doctrine alignment) are now orphaned on that remote branch. They were ALREADY in master per `git rebase` reporting (`skipped previously applied commit`) — so no semantic loss, but the orphan branch on origin still shows them. The heartbeat orphan-sweep will surface this.

**If wrong:** revert with `git revert <merge-sha>`; the substrate is self-contained and the wiring milestones can re-derive against a different shape. Or amend the scope (e.g. demand the full M3/M6/M8 wiring before merge).

---

### PD-002 — Remote branch divergence: `origin/claude/review-rmp-spec-8a7VX` vs `origin/master`

**Decision (autonomous):** continuing all work on `master`. Will not touch `claude/review-rmp-spec-8a7VX` without your direction.

**Background:** at session start, `git status` reported:
> Current branch: master
> Main branch (you will usually use this for PRs): claude/review-rmp-spec-8a7VX

The remote HEAD is `origin/claude/review-rmp-spec-8a7VX` (GitHub default). All orchestrator + agent work this session has gone to `master`. The two branches diverged: T19 framework-magic-reconciler accidentally pushed its commits (`c53ed1e`, `76769d9`, `175632b`, `209dee8`) to `claude/review-rmp-spec-8a7VX` (because the worktree was created from that branch). I detected this on T19's completion notification and cherry-picked those commits onto `master` (`1a897e8`, `7f5944e`, `a52acfc`).

**The orphan branch is now stale.** It contains a parallel history with semantically-equivalent commits up through the doctrine expansion, but lacks everything master has past `ea3d40e` (M1 PASS, meta-subscribe research, M2 fixes, README updates, T19 cherry-picks themselves, etc.). Approximately 20+ commits.

**Options:**
- **(a)** Merge `master` into `claude/review-rmp-spec-8a7VX` (fast-forward-able if I rebase the orphan onto master first). Keeps the remote-default branch name with all-the-things.
- **(b)** Set GitHub default to `master` and delete `claude/review-rmp-spec-8a7VX`. Cleaner; breaks any URLs / external references to that branch name.
- **(c)** Leave both — `claude/review-rmp-spec-8a7VX` stays as a historical snapshot of pre-session state; master is the active branch.

**Recommendation:** (a). Preserves all branch references, no rename impact, keeps the historical name. If you prefer (b) it's a one-liner.

**While you decide:** all agents have been instructed (and the heartbeat reinforces) to push to `master`. Future T19-style accidents will be caught faster — I added a `git branch --show-current` check to spot drift earlier.

---



---

## Resolved (user acked or superseded)

### PD-001 (resolved 2026-05-18) — Doctrine vocabulary collision

**User picked option (b):** expand `docs/product-spec/overview-and-dx.md` §1.5 to formally absorb the three additional load-bearing rules (errors-never-FFI / capabilities-report / reactivity-≤60Hz) as named doctrines D6, D7, D8.

Product-spec now has D0–D8 with an explicit "two kinds" distinction:
- **D0–D5: policy doctrines** — user-facing semantics (kernel-boundary, best-effort rendering, negentropy-first, outbox-automatic, single-writer-per-fact, snapshots-bounded).
- **D6–D8: substrate invariants** — runtime / FFI / hot-path constraints (errors-never-FFI, capabilities-report, reactivity-contract).

Conflicts still resolve in listed order (D0 wins over D8). README aligned. T19 framework-magic-reconciler in flight will absorb D0–D8 into the framework-magic docs (sending them an updated brief alongside this commit).
