---
type: research-record
date: 2026-06-18
session: 156aa64b-42e1-4d3b-96ce-25b31fc06fec
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/156aa64b-42e1-4d3b-96ce-25b31fc06fec.jsonl
source_lines: 95-389
agent_attribution: Sonnet: worktrees for DM bunker + NIP-65
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Architectural problems audit of HEAD ranking top issues, classifying each as live/fixed/never with file:line evidence and git worktree inspection, verdicts include PARTIALLY-ADDRESSED/NOT-IN-FLIGHT/FIXED/NEVER-EXISTED"
captured_at: 2026-06-18T05:04:55Z
---

Architectural problems audit of HEAD ranking top issues, classifying each as live/fixed/never with file:line evidence and git worktree inspection, verdicts include PARTIALLY-ADDRESSED/NOT-IN-FLIGHT/FIXED/NEVER-EXISTED

---

User: [Agent task result: Agent "Sonnet: worktrees for DM bunker + NIP-65" completed]
I now have a complete picture. Here is the verdict:

---

## Problem 1: ADR-0026 DM Bunker Path Inert / NIP-17 Bunker DM

**PARTIALLY-ADDRESSED — two in-flight branches, neither closes the bunker gap**

**Branch: `pr/signer-seam-E-v2`** (2 commits ahead of master, worktree at `/Users/pablofernandez/Work/nostr-multi-platform/.claude/worktrees/agent-acc46fbc426663602`, no open PR yet)

This is the most substantive work on Problem 1. It adds:
- `nmp_nip59::SignerForSeal` trait + `gift_wrap_with_signer` function in `crates/nmp-nip59/src/signer_seal.rs`
- `IdentityRuntime::active_signer_for_seal()` in `nmp-core`
- `dm.rs` re-routed to call `gift_wrap_with_signer` via `active_signer_for_seal` instead of `active_local_keys`
- D13 doctrine-lint rule enforcing the seam (`crates/nmp-testing/bin/doctrine-lint/rules/d13.rs`)

**Bunker gap status: still open.** The three references to `RemoteSignerHandle` in `signer_seal.rs` are comment-only (doc text). The `active_signer_for_seal()` call returns `None` for remote accounts, `dm.rs` surfaces a toast and returns early. The Phase 2 `RemoteSignerHandle → SignerForSeal` adapter is explicitly deferred in the comments. The seam is structurally correct and the infrastructure for plugging in bunker support is in place, but bunker DMs still fail with a toast.

**Branch: `codex/nip17-dm-relay-fail-closed`** (1 commit, open as PR #228)

Separate concern: it fixes the DM send routing to fail closed when `kind:10050` is missing (no fallback to Content relays). It does NOT use `gift_wrap_with_signer` — its `dm.rs` still calls `active_local_keys` / `nmp_nip59::gift_wrap` (the old raw-key path). These two branches are **not integrated** — they address orthogonal aspects of the DM send path and will conflict on `dm.rs`.

**Duplication/conflict flag**: `pr/signer-seam-E-v2` and `codex/nip17-dm-relay-fail-closed` both modify `crates/nmp-core/src/actor/commands/dm.rs` and the doctrine-lint rules (`d12.rs` deleted in both). They will have merge conflicts; whichever lands second will need to re-incorporate the other's changes.

---

## Problem 2: NIP-65 Publish UI Missing (kind:10050)

**NOT-IN-FLIGHT**

No branch ahead of master adds a `PublishDmRelayList` `ActorCommand`, an `nmp_app_publish_dm_relay_list` FFI function, or any iOS UI to let the user publish their own `kind:10050` relay list. The `codex/nip17-dm-relay-fail-closed` PR (#228) enforces fail-closed on the *send* side (refusing to route DMs when the recipient's `kind:10050` is missing) and the `codex/worker4-publish-relay-normalization` PR (#227) touches publish engine relay normalization — but neither adds the publish UI that would make Chirp users visible as DM recipients. There is zero in-flight work on this.

---

## Summary Table

| Problem | Status | Branch(es) |
|---|---|---|
| ADR-0026 bunker DM seam (Phase 2) | PARTIALLY-ADDRESSED | `pr/signer-seam-E-v2` (seam built, remote adapter deferred); `codex/nip17-dm-relay-fail-closed` (fail-closed routing, orthogonal) |
| kind:10050 publish UI | NOT-IN-FLIGHT | — |

Assistant: Got the DM bunker + NIP-65 report. Holding it while the other 4 finish.

User: [Agent task result: Agent "Sonnet: worktrees for correlation_id + nip29" completed]
I now have a complete picture. Let me summarize.

---

## Survey Results

### Problem 1: correlation_id / spinner round-trip broken

**Verdict: ALREADY MERGED — not in-flight, already on master.**

Three PRs addressed this in sequence, all merged on 2026-05-21:

- **PR #86** (`fix/correlation-id-round-trip`, commit `54f4ddf0`) — directly named in the task. Added `ActionModule::preferred_action_id()` so `PublishModule` returns `event.id` instead of a minted id for pre-signed Publish variants. The commit message explicitly states: *"dispatch_action previously returned a freshly-minted 32-hex id … while last_action_result reported the publish engine's PublishHandle (== event.id, a 64-hex sha256). The two namespaces never matched."*

- **PR #89** (on `chore/remove-dead-wire-fields`) — threaded `correlation_id` through `ActorCommand` for `PublishNote` path (the remaining gap noted in PR #86's commit).

- **PR #117** (`fix/action-result-broken-promise`, commits `c7b35649` + `8e9be86d`) — closed broken-promise on sign-step and engine-level errors (the `correlation_id` was being leaked on failure paths without pushing a terminal verdict).

Current master state: `publish/engine.rs` has `correlation_id_override: Option<String>` threading the host-received id through to `action_results`, with `LastTerminal::correlation_id` always equal to the id the host received from `dispatch_action`. No worktree or open branch touches this.

---

### Problem 2: nip29.* namespace violates nmp.* convention

**Verdict: ALREADY MERGED — not in-flight, already on master.**

- **PR #103** (`fix(nip29): snake_case admin action namespaces`, commit `eb0a9230`) — fixed the CamelCase vs snake_case issue from review #34.

- **PR #153** (`pr/nip29-namespace-rename-B`, commits `f5348c44` + `e7111ffc`) — renamed all bare `nip29.*` action `NAMESPACE` constants to `nmp.nip29.*` prefix, and added D9 doctrine-lint rule to enforce it going forward.

Current master state: All `NAMESPACE` constants in `crates/nmp-nip29/src/action/` now read `"nmp.nip29.post_chat_message"`, `"nmp.nip29.discover"`, `"nmp.nip29.join"`, etc. The D9 lint (in `crates/nmp-testing/bin/doctrine-lint/rules/d9.rs`) actively bans any bare `nip29.*` NAMESPACE constant. There is no open branch or worktree touching this.

**Note on remaining `"nip29.*"` strings in master:** There are still `"nip29.group_chat"` and `"nip29.discovered_groups"` strings used as *snapshot projection keys* (not action NAMESPACE constants) in `apps/chirp/nmp-app-chirp/src/ffi.rs` and iOS bridge files. D9 does not ban these (it only flags `NAMESPACE` constants). The `"nip29.discover"` string in `crates/nmp-nip29/src/interest.rs:80` is used as a stable hash seed for `InterestId`, not a namespace — renaming it would silently break interest de-duplication. No in-flight branch is addressing these snapshot keys.

---

### Duplication / Conflict

None. The three branches for Problem 1 and two branches for Problem 2 were sequenced correctly and are all already on master. No open PRs or active worktrees duplicate this work.

Assistant: Two of five back. correlation_id and nip29 namespace are both already merged. Continuing to wait on the remaining three.

User: [Agent task result: Agent "Sonnet: worktrees for NIP-57, Chirp LoC, dormants" completed]
Now I have a comprehensive picture. Let me compile the final findings.

---

## Survey Results

### Problem 1: NIP-57 Zaps Inert

**CLOSED-WITHOUT-MERGE (branch: `pr/nip57-lnurl-zap-C`)**

This is the most nuanced finding. There were *two* opposing moves in rapid succession on May 21:

- **PR #154 (`chore/delete-dormant-zap-action`) — MERGED** at 16:24: Deleted the dormant `ZapModule` ActionModule + executor from `nmp-nip57/src/action.rs` (405 lines removed), cleaned `apps/chirp/nmp-app-chirp/Cargo.toml` comment. Rationale: shipped-but-inert, LNURL HTTP path was a stub, no bolt11 ever fetched.

- **PR #159 (`pr/nip57-lnurl-zap-C`) — CLOSED (not merged)** at 16:44: Built a full in-process LNURL executor — new `nmp-nip57/src/lnurl.rs` (~520 LoC), `fetch_invoice()` using `ureq` (2-step LNURL-pay flow per LUD-06 + NIP-57), iOS Zap button wired, `ZapModule` re-registered. 4 commits. This was CLOSED 20 minutes after #154 merged, suggesting it was superseded or conflicted with the delete. The commits (`7837bd6e`, `9ce3ede1`, `2365b45e`, `7c46fa75`) exist only on the branch — **not on master**.

No other in-flight worktree or branch contains zap/LNURL work. The current master state: `nmp.zap` is deregistered, `action.rs` deleted. The LNURL approach *was* decided in PR #159 (sync/blocking `ureq` on a spawned worker thread, D8-safe), but that PR was closed rather than merged.

**Verdict: NOT-IN-FLIGHT.** The delete landed; the re-implementation PR was closed. Zero active branches carry zap work forward.

---

### Problem 2: Chirp Thin-Shell Violation

**PARTIALLY-ADDRESSED — multiple merged PRs, no open PR**

Extensive thin-shell work landed today (May 21):
- **PR #215 (`codex/dm-kernel-thinshell`) — MERGED**: Moved Chirp DM relay lifecycle into Rust (`KernelBridge.swift` −106 lines, `KernelModel.swift` −8 lines).
- **PR #177 (`agent/dm-substrate-thin-shell`) — MERGED**: Pre-classified DM `is_outgoing` + chronological order in Rust.
- **PRs #189, #187, #190, #191, #185, #181** — MERGED: Series of "move X to Rust" micro-PRs targeting `aim.md §4/§6` anti-patterns (SF Symbol pre-classification, ProfileView switch elimination, SettingsHub/Search formatting, MediaKind enum, bunker handshake flags, Marmot Swift substrate).
- **PR #209 (`pr/native-hygiene-L`) — MERGED**: KernelModel collapse, Codable createAccount.

**NmpHighlighter**: Already deleted from the repo (`ios/` only contains `Chirp/`). The concern from review #6 (39K LoC) is moot — the app no longer exists.

**Current Chirp LOC on master**: 11,585 Swift lines total. This is higher than review #15's 8,660 figure, which means the app has grown despite the thin-shell migrations. No open PR is explicitly targeting a LOC cap or bulk logic extraction. The worktree sweep shows many agents doing micro-migrations (pre-classifying fields in Rust), which chip away at logic in Swift without being a coordinated LOC-reduction program.

**Verdict: PARTIALLY-ADDRESSED** — many merged PRs move logic Rust-ward, but 11,585 LOC is still 34% above the review #15 regression watermark of 8,660, and no open branch is explicitly targeting bulk extraction. The `chirp-nmp-thin-shell-policy` branch (4 commits ahead of an old merge base, not recently updated) adds CI drift detection but doesn't remove Swift LoC.

---

### Problem 3: Dormant ActionModule Impls

**ADDRESSED-IN-FLIGHT → LANDED**

- **PR #200 (`fix/delete-dormant-marmot-action-modules`) — MERGED** at 18:59: Deleted 6 dormant group-op ActionModule impls from `crates/nmp-marmot/src/action/actions.rs`. Diff confirms deletion of `impl ActionModule for PublishKeyPackageAction` and rename of its NAMESPACE (from `nmp.marmot.publish_key_package` to `marmot.publish_key_package`), plus deletions in `domain/mod.rs` (4 DomainModule NAMESPACE consts), `projection/`, and `view/`.

- **PR #174 (`worktree-agent-aa3847a969caac2ed`) — MERGED** at 17:44: `chore(nmp-nip77): delete dormant RunSync ActionModule stub`.

- **Review #37's delete-12 admin/membership directive**: The 6-deletion PR covers the Marmot side. The admin/membership NIP-29 executors were targeted by review #37; no separate open branch specifically for those was found.

**Current state on master**: Only `PublishKeyPackageAction` remains in `nmp-marmot/src/action/`. The `DomainModule` NAMESPACE consts in `domain/mod.rs` are `doctrine-allow: D9` (LMDB storage prefixes, data migration required — not deletable without migration).

**Verdict: ADDRESSED-IN-FLIGHT (landed).** PR #200 merged 6 Marmot deletions. nmp-nip77 stub also deleted. The review #37 full 12-deletion mandate is partially complete (6 done, remainder not in any open branch).

---

### Conflicting Work

The delete-then-re-add sequence on NIP-57 (PR #154 deletes, PR #159 re-adds but was closed) is the most significant conflict. PR #159 was fully built and ready but was closed — likely because it conflicted on the same files that PR #154 just modified, or because the orchestrator decided to defer. No branch is currently *adding* new dormant modules, but the `codex/chirp-web-v1-proof` (open PR #219) adds a WASM-based Chirp bridge which would be a thin-shell concern if it grows.

Assistant: Three of five back. Waiting on the action seam scout and the master Opus audit.

User: [Agent task result: Agent "Sonnet: worktrees for action seam" completed]
Now I have all the information needed. Here is the complete scouting report:

---

## Scouting Report

### Problem 1: Dual action seam foot-gun (register_action_module + register_action_executor)

**Verdict: ADDRESSED-IN-FLIGHT — ADR only, no code implementation yet.**

- **ADR-0027** (`docs/decisions/0027-unified-action-module-trait.md`) was authored in worktree branch `worktree-agent-a47a6d2abfbeabf26` and shipped as **PR #196**, which is already **MERGED** to master.

- The ADR (Status: Proposed) documents exactly the fix: extend `ActionModule` with a required `fn execute(...)` method that returns an `ActorCommand`, so `NmpApp::register_action_module<M>()` becomes a single generic call that wires both halves atomically. The current `wire_action!` macro in `apps/chirp/nmp-app-chirp/src/ffi.rs:67-88` is called out as the paper-over that the trait change would make unnecessary.

- Key ADR quote: *"The broader 'consolidate all write paths' question — `publish_signed_event` vs. `dispatch_action` vs. the 36 `ActorCommand` variants that bypass the action seam — is out of scope. This ADR is scoped to the **registration** seam only."*

- No code implementation branch found. The trait (`crates/nmp-core/src/substrate/action.rs`) still has only `fn start(...)` — no `fn execute(...)`. The two-call split (`register_action_module` at `ffi/mod.rs:690` + `register_action_executor` at `ffi/mod.rs:663`) is unchanged on master.

- No second branch is working on this in parallel — no duplication risk.

**Summary:** ADR written and merged; implementation PR not yet opened.

---

### Problem 2: dispatch_action bypassed by ActorCommand variants

**Verdict: NOT-IN-FLIGHT.**

- ADR-0027 explicitly marks this as **out of scope** in its "Out of scope" section: *"the 36 `ActorCommand` variants that bypass the action seam… is a separate, larger architectural conversation."*

- Current master has approximately 44 `ActorCommand` variants (confirmed by grepping `crates/nmp-core/src/actor/mod.rs`). Among them, variants like `PublishNote`, `PublishProfile`, `React`, `Follow`, `Unfollow`, `SendGiftWrappedDm`, `WalletPayInvoice`, etc. all bypass `dispatch_action` entirely.

- No branch name, commit message, or open PR was found proposing to route any of these through `dispatch_action`. The only historical movement was `publishProfile` which was routed through `dispatch_action` in a past commit (dc0e28a7, PR #95), but that was an isolated fix, not a systematic campaign.

- No in-flight ADR for this problem either (ADR directory searched: only ADR-0027 touches the subject, and explicitly defers it).

---

### No duplication between candidates

Only one branch (now merged as PR #196) touched Problem 1 at all. No branch touches Problem 2 at all.

Assistant: Four of five back. Waiting on the Opus master audit.

User: [Agent task result: Agent "Opus: major arch problems audit" completed]
iOS DOES consume `actionResults` and matches on `correlationId` (KernelModel.swift:433-436). The round-trip is end-to-end wired. #3 is FIXED.

Now I have enough evidence. Here's the punch list:

---

# Architectural Audit — nostr-multi-platform @ 997aac20 (2026-05-22)

## Tier 1 — Live correctness bugs

### A1. ADR-0026 bunker DM path is inert (silent feature gate)
**Status: LIVE.** `crates/nmp-core/src/actor/commands/dm.rs:110-117`:
```
let Some(keys) = identity.active_local_keys() else {
    kernel.set_last_error_toast(Some(
        "cannot send DM: gift-wrap needs a local key — remote (bunker) \
         signers are not yet supported for NIP-17"
```
**Why it matters:** Bunker (NIP-46) users cannot send NIP-17 DMs at all. ADR-0026's `nip44_encrypt` seam is documented but unwired — the executor still requires raw `nostr::Keys`. Failure is a toast, easy to miss in green CI.

---

## Tier 2 — Shipped-but-inert (registration without consumer)

### B1. NIP-65 publish UI — `nmp.nip17.publish_relay_list` action has zero callers
**Status: LIVE (consumer gap).** Registered at `apps/chirp/nmp-app-chirp/src/ffi.rs:678-684`. Zero hits for `publish_relay_list` / `PublishDmRelayList` in `ios/Chirp/**.swift`.
**Why it matters:** Every Chirp user is invisible to other clients trying to DM them — they never publish a kind:10050. This is the "consumer pipe" pathology from review #36: action shipped, action_stages plumbed, no UI button calls it.

### B2. NIP-57 zaps — no `ZapAction` exists, much less a registered one
**Status: LIVE.** `crates/nmp-nip57/src/lib.rs` exports only `ZapsDomain`/`ZapsView` (passive ingest). No `impl ActionModule` anywhere in `nmp-nip57`. No `nmp.zap` namespace registered. HttpCapability deleted (memory accurate).
**Why it matters:** Zaps inert end-to-end. ADR-0024 (LNURL HTTP path) never landed; no zap can be sent.

### B3. Dormant `PublishKeyPackageAction` (NIP-29 / Marmot)
**Status: LIVE, smaller than review #49 claimed.** `crates/nmp-marmot/src/action/actions.rs:25-26` declares `impl ActionModule for PublishKeyPackageAction { const NAMESPACE: &'static str = "nmp.marmot.publish_key_package"; … }`. No `wire_action!` / `register_action_module` call in `apps/chirp`.
**Why it matters:** Review #49 said "7 dormant Marmot impls"; at HEAD only 1 exists. Of 10 total `impl ActionModule`, 8 are wired (publish, 5 NIP-29, 2 NIP-17), 1 is fixture-todo, 1 dormant. The dormant impl is still scope creep but the regression is narrower than reviews suggested.

---

## Tier 3 — Structural foot-guns / debt

### C1. Dual-action seam foot-gun (doctrine #3 not type-enforced)
**Status: LIVE.** Two API calls register one logical action; the `wire_action!` macro `apps/chirp/nmp-app-chirp/src/ffi.rs:70-95` papers over the duplication for typed actions but the underlying API requires both `register_action_module` AND `register_action_executor`. Core docstring at `crates/nmp-core/src/ffi/mod.rs:679-682` admits: *"Registering only one half leaves the namespace partially wired"*.
**Why it matters:** Untyped registrations (e.g. `register_action_module(NS_REACT, …)` at `ffi.rs:563,579,592`) still split the validator from the executor — `register_action_executor` is silently called elsewhere or omitted. A typo in one of the two namespace strings is a runtime-only failure.

### C2. dispatch_action seam bypassed by ~30 of 43 ActorCommand variants
**Status: LIVE.** 43 `ActorCommand::Foo` arms in `crates/nmp-core/src/actor/dispatch.rs` (verified by grep). ~30 direct `app.send_cmd(ActorCommand::…)` FFI sites in `crates/nmp-core/src/ffi/{timeline,wallet,identity,publish,mod}.rs`. Only ~11 namespaces go through `dispatch_action` (5 NIP-29, 2 NIP-17, 3 chirp.*, 1 nmp.publish).
**Why it matters:** The substrate seam — the load-bearing "host-extensibility" thesis — is bypassed for two-thirds of kernel commands. Per-app FFI continues to grow in parallel.

### C3. Chirp thin-shell rule violated on BOTH sides
**Status: LIVE.** 
- Swift: 12,476 LOC across 52 files (`ios/Chirp/Chirp/Bridge/KernelBridge.swift` alone is 1,821 LOC). Thin-shell rule: "ZERO logic allowed."
- Rust: `apps/chirp/nmp-app-chirp/src/ffi.rs` is 1,374 LOC; `follow_list.rs` (297) is a `KernelEventObserver` *projection impl*, `dm_runtime.rs` (253) is runtime state. The Rust shell hosts real logic, not just C-ABI delegators.

**Why it matters:** Chirp is supposed to be a reusability proof; both sides have become a second home for app-shaped projections.

### C4. Single-actor channel — backpressure observed, not enforced
**Status: LIVE.** `crates/nmp-core/src/ffi/mod.rs:347-353` — *"the command channel is an unbounded `std::sync::mpsc::channel()`"*. `queue_depth` is an `AtomicU64` observability counter (legitimate, per memory). No bounded channel; no shed-load policy.
**Why it matters:** Memory expansion under pathological producer loops; FFI broker bypasses the counter (lower-bound only). Review #1's "no backpressure" risk is mitigated at the metric level, not at the control level.

### C5. `nip29.*` snapshot-projection keys leak (action namespaces clean)
**Status: PARTIAL.** Action namespaces are all `nmp.nip29.*` (`crates/nmp-nip29/src/action/*.rs`). Bare `nip29.*` survives only as:
- snapshot projection keys: `apps/chirp/nmp-app-chirp/src/ffi.rs:298,368` (`"nip29.group_chat"`, `"nip29.discovered_groups"`).
- interest label: `crates/nmp-nip29/src/interest.rs:80` (`"nip29.discover"`).

Both sites are OUTSIDE `nmp-core`, so doctrine-lint D9 (which bans `nip29` in `nmp-core`) does not catch them.
**Why it matters:** Convention drift in snapshot keys means iOS code paths (`KernelBridge.swift:941: case dmInbox = "nip17.dmInbox"`) carry the same bare prefix.

---

## Tier 4 — Trend regression

### D1. Mutex count up 108 → 125 (~16% growth since review #19)
**Status: LIVE.** `grep -rn "Arc<Mutex<\|Arc::new(Mutex::new" crates/nmp-core/src` returns 125 (excluding tests).
**Why it matters:** Memory notes `actor_queue_depth` and `pending_mls_autopublish` are legitimate non-Mutex slots, but 125 still indicates ongoing Mutex-as-default for kernel state.

---

## Fixed / Never-existed

### F1. correlation_id / spinner round-trip — FIXED
- `crates/nmp-core/src/publish/engine.rs:79` carries `correlation_id_override: Option<String>`.
- `crates/nmp-core/src/ffi/action.rs:439-447` returns `{"correlation_id":<minted-id>}`.
- iOS consumer `ios/Chirp/Chirp/Bridge/KernelModel.swift:433-436` matches `terminal.correlationId` from `actionResults`.

Round-trip is end-to-end wired for `PublishNote`. The 2026-05-21 code-grounded audit was right; reviews #24/#25/#29/#30 were stale.

### F2. KernelSnapshot 76-field claim — NEVER EXISTED at current HEAD
`crates/nmp-core/src/kernel/types.rs:653-757`: only **15** typed `pub(super)` fields. Identity, publish, views, wallet, bunker_handshake are all in the `projections: HashMap<String, Value>` map. Review #6 was either stale or counted differently.

### F3. `action_stages` substrate (PR-G) — wired end-to-end
`ios/Chirp/Chirp/Bridge/KernelBridge.swift:1411-1431` decodes the projection; `ios/Chirp/Chirp/Bridge/KernelModel.swift:436` calls `kernel.ackActionStage(terminal.correlationId)`. Live consumer, not shipped-inert.

---

## Top 3 recommended next moves (no edits made)

1. **A1 first — wire `gift_wrap_with_signer` for bunker via ADR-0026's `RemoteSignerHandle::nip44_encrypt`.** This is the only Tier-1 live correctness gap and silently excludes bunker users from NIP-17.
2. **Add a Swift "publish DM relay list" call site** before adding any more NIP-17 features — otherwise B1 (no kind:10050 publishes) makes B3/A1 cosmetic anyway: nobody can be DM'd.
3. **Collapse the dual-action seam (C1) into one `ActionModule` registration method** — type-enforces doctrine #3 and removes the wire_action! workaround.
