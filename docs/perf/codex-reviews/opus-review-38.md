# Opus Direction Review #38 — The consumer pipe converted; now hold the line

Date: 2026-05-21
Reviewer: Opus (architectural direction audit)
Scope: NMP `dispatch_action` seam + Chirp FFI + NIP-29 post-deletion state + NIP-59/NIP-17 readiness + the Marmot FFI cluster + `nmp.zap`/ADR-0024. Ground truth: `git log origin/master`, `git show origin/master:<path>`, `git ls-tree`, direct file reads.

---

## Note on numbering and ground truth

Two corrections to the brief, stated up front because they change the verdict:

1. **There is no `opus-review-37.md` on disk or in git history.** The latest review present is `opus-review-36.md`. This review (#38) is written against **#36 as the real predecessor**. The "#37 said NIP-17 is highest-leverage" framing in the brief is treated as a forward intent, not a citation.
2. **The brief says `GroupChatView.swift` is "in a worktree PR (not yet merged)." It is merged.** It landed as **PR #115** (`25d03390 feat(chirp): NIP-29 group-chat screen — first seam consumer`) on `origin/master`. This worktree's `HEAD` (`62ebbf1d`) is exactly one commit behind `origin/master`. All claims below are verified against `origin/master`, not the stale worktree checkout. This matters: #115 is the single most important fact in this review, and a review written against the worktree HEAD would miss it entirely and get the trajectory wrong.

---

## TL;DR — read this first

- **The pathology #25–#36 kept naming — "working merge pipe, broken consumer pipe" — has, for the first time, measurably reversed.** #36's verdict was: build one NIP-29 screen, delete the 14 dormant executors, on a 2-cycle clock. **Both happened in one cycle.** PR #113 deleted 2,369 lines of dead NIP-29 admin/membership/view/domain code. PR #115 wired `GroupChatView` + `GroupChatStore` + `GroupChatBridge` — a navigable screen (pushed from `MarmotGroupsView.swift:98`) that reads the `nip29.group_chat` snapshot projection and writes via `nip29.post_chat_message`. This is the first NIP-crate `ActionModule` with a real Swift caller. Give this credit before reading the rest.
- **Live:inert is now 6:4 — the best ratio in the review series.** Live (Swift caller exists): `nmp.publish`, `chirp.react`, `chirp.follow`, `chirp.unfollow`, `nip29.post_chat_message`, plus the `nip29.group_chat` read projection. Inert (registered, zero Swift caller): `nip29.react_in_group`, `nip29.comment_in_group`, `nmp.zap`, and `HttpCapability`. #36 measured 4:6. The deletion did not merely shrink the inert pile — PR #115 *converted* a surface from inert to live. Trajectory: genuinely improving.
- **NIP-17 DMs are smaller than they look, but not free.** `nmp-nip59` already ships the gift-wrap crypto (`gift_wrap`/`unwrap_gift_wrap`, `wrap.rs`). What does *not* exist: a kind:14 chat-rumor builder, a `nmp.dm.send` action namespace, an inbound kind:1059→kind:14 projection, and a Swift screen. Critically, `WelcomeUnwrapModule` (`nip59/domain/welcome_unwrap.rs:47`) is itself a **dormant `DomainModule`** — its own docstring admits "the actual decryption + MDK dispatch is performed by the actor layer," i.e. it declares an ingest kind nothing drives. NIP-17 ingest cannot copy that pattern; it needs a real `KernelEventObserver` projection like `GroupChatProjection`.
- **The Marmot FFI cluster is structural debt and the ADR #36 asked for never appeared.** `grep -rln -i marmot docs/decisions/` returns nothing. The cluster is 8 bespoke `nmp_app_chirp_marmot_*` C symbols, and one of them — `nmp_app_chirp_marmot_dispatch` (`apps/chirp/nmp-app-chirp/src/marmot/ffi.rs:384`) — is a **parallel action-dispatch envelope** that bypasses `nmp_app_dispatch_action` entirely. That is the exact "silent seam erosion" #36/#37 warned about, now undocumented for another cycle.
- **`nmp.zap` is inert for the 5th consecutive review and ADR-0024 is still 0/5.** Verified: zero `CapabilityResultReady`, zero `deliver_capability_result`, no `ActionModule::resume`. There is no path to a working zap that skips ADR-0024 — the LNURL POST is a hard async dependency. **Recommendation: delete the `register_nip57_actions` call (`ffi.rs:486-488`) this cycle.** It is a 3-line deletion that converts a shipped lie into honest absence.

---

## 1. The consumer-pipe status after the deletions

**The count, verified on `origin/master`:**

| Surface | Registration | Swift consumer | User impact |
|---|---|---|---|
| `nmp.publish` | ✅ | ✅ `KernelBridge.publishNote` / `publishProfile` | **Live** |
| `chirp.react` | ✅ | ✅ `KernelBridge.react` | **Live** |
| `chirp.follow` | ✅ | ✅ `KernelBridge.follow` | **Live** |
| `chirp.unfollow` | ✅ | ✅ `KernelBridge.unfollow` | **Live** |
| `nip29.post_chat_message` | ✅ `ffi.rs:453` | ✅ `GroupChatBridge.postChatMessage` → `GroupChatStore.sendMessage` | **Live** (PR #115) |
| `nip29.group_chat` (read) | ✅ `ffi.rs:270` | ✅ `GroupChatStore.apply` ← `KernelModel` | **Live** (PR #115) |
| `nip29.react_in_group` | ✅ `ffi.rs:454` | ❌ no Swift caller (`git grep react_in_group origin/master -- '*.swift'` → empty) | **Inert** |
| `nip29.comment_in_group` | ✅ `ffi.rs:455` | ❌ no Swift caller | **Inert** |
| `nmp.zap` | ✅ `ffi.rs:487` | ❌ no Swift caller | **Inert** + no satoshi moves even if called |
| `HttpCapability` | ✅ Rust seam + `HttpCapability.swift` | ❌ no Rust executor calls `HttpCapabilityWiring` | **Inert** |
| `nmp.nip57.zaps` (DomainModule) | ✅ `nip57/domain.rs:10` | ❌ no host registers `ZapsDomain` | **Inert** (additional, see §5) |
| `nip59.welcome_unwrap` (DomainModule) | ✅ `nip59/domain/welcome_unwrap.rs:48` | ❌ no driver — actor does the work directly | **Inert trait shell** |

**Live: 6. Inert: 4 action/capability surfaces (+2 dormant `DomainModule` trait shells).**

**The trajectory verdict: this is the first review where the answer is unambiguously "improving."** #36 measured 4 live : 6 inert. Today it is 6 live : 4 inert. The deletion (PR #113, −2,369 lines) did the negative half of #36's instruction; PR #115 did the positive half — it *converted* `nip29.post_chat_message` and `nip29.group_chat` from inert to live, with a navigable screen. This is not "the inert pile got smaller." It is "two surfaces became real." The 14-executor `nip29.*` cluster #36 raged about is gone; what remains is 3 chat executors, 2 of which have a screen one Swift call site away.

**The discipline that produced this should be named and repeated:** #36 gave a concrete fork ("build one screen / delete 14, on a 2-cycle clock") and the project executed *both arms in one cycle*. The lesson is that vague "keep them, they're cheap" instructions produce nothing for 5 reviews; a concrete delete-or-wire fork with a deadline produced a result in one. Apply that exact shape to the remaining 4 inert surfaces.

---

## 2. NIP-17 DMs — how hard is it really?

#36/#37 named NIP-17 the next high-leverage capability. Reading `crates/nmp-nip59/src/` in full, here is the honest delta.

**What already exists (do NOT count as work):**
- `gift_wrap(sender, receiver, rumor, expiration)` — seal (kind:13) + gift-wrap (kind:1059). `nip59/wrap.rs:34`.
- `unwrap_gift_wrap(receiver, gift_wrap)` — verify seal, extract rumor. `nip59/wrap.rs:53`.
- `UnwrappedGift { sender, rumor }` — `nip59/wrap.rs:18`.
- The Marmot path already subscribes to a kind:1059 `#p=self` inbox (`nmp-marmot/interest.rs:61`) — the *interest/routing* shape for a gift-wrap inbox is a solved, copyable pattern.

**What does NOT exist (the real work):**
1. **A kind:14 chat-rumor builder.** NIP-17 DMs are a kind:14 (or kind:15 for files) *rumor* — an unsigned event — wrapped by `gift_wrap`. There is no `nmp-nip17` crate and no kind:14 `UnsignedEvent` builder. New crate `nmp-nip17` (or a module in an existing one): ~1 builder function, typed `DmInput { recipient_pubkey, content, reply_to: Option<String> }`.
2. **A `nmp.dm.send` action namespace + executor.** NIP-17's send is *not* one `PublishUnsignedEventToRelays` — it is **one gift-wrap per recipient AND one self-copy** (the sender must gift-wrap to their own pubkey to see their sent messages). The executor builds the kind:14 rumor, calls `gift_wrap` twice (recipient + self), and emits two `PublishUnsignedEventToRelays` commands pinned to the recipient's and sender's DM-relay (kind:10050) sets. This needs signer access — `gift_wrap` takes `nostr::Keys` — so the executor must reach the actor's signer, not just `send(cmd)`. **This is the one genuinely new architectural piece:** the existing `wire_action!` executors are pure `action_json → ActorCommand` functions with no key access. A DM-send executor needs the actor to perform the gift-wrap at command-handling time (a new `ActorCommand::SendGiftWrappedDm { rumor, recipients }` is the clean shape — keep crypto on the actor thread, D7).
3. **An inbound kind:1059 → kind:14 projection.** A `KernelEventObserver` like `GroupChatProjection` that, on each kind:1059, calls `unwrap_gift_wrap` (needs the receiver's `Keys` — same signer-access problem) and, if the rumor is kind:14, accumulates a `DmMessage` list keyed by conversation pubkey. **Do NOT model this as a `DomainModule` like `WelcomeUnwrapModule`** — that type is a dormant trait shell that drives nothing (§1). Copy `GroupChatProjection`'s `KernelEventObserver` + `register_snapshot_projection` shape exactly; it is the proven live pattern.
4. **A Swift `DmConversationView` + `DmListView` + bridge.** Mechanically identical to PR #115's `GroupChatView`/`GroupChatStore`/`GroupChatBridge`. ~Copy-and-adapt.

**Realistic minimum delta to a working DM send+receive:** new `nmp-nip17` (kind:14 builder + typed input), one `ActorCommand::SendGiftWrappedDm` variant + actor arm (the signer-access piece — the only non-trivial design decision), one inbound `DmInboxProjection` (`KernelEventObserver`), one `nmp_app_dispatch_action` namespace `nmp.dm.send`, one snapshot key `nip17.dm_inbox`, and two Swift screens. **It is ~1.5× the PR #115 effort** — bigger than PR #115 only because of the actor signer-access seam, which is genuinely new and worth its own small ADR. Everything else is a direct copy of the now-proven NIP-29 screen pattern.

**One sequencing caveat:** the signer-access seam (item 2) is shared infrastructure NIP-17 *and* a real Marmot-via-`dispatch_action` future both need. Build it deliberately; do not let it be a Marmot-style bespoke side-channel.

---

## 3. The Marmot FFI cluster — intentional exception or structural debt?

**The ADR #36 explicitly recommended does not exist.** `grep -rln -i marmot docs/decisions/` → empty. Twenty-four ADRs in `docs/decisions/`, zero mention Marmot.

**The cluster is 8 bespoke C symbols** (`apps/chirp/nmp-app-chirp/src/marmot/`):
`nmp_app_chirp_marmot_register`, `_register_active`, `_snapshot`, `_group_messages`, `_dispatch`, `_string_free`, `_unregister`, `_fetch_key_packages` — plus 3 more `nmp_app_chirp_identity_*` symbols in `marmot/identity.rs`.

**This is structural debt, and the most acute symptom is `nmp_app_chirp_marmot_dispatch`** (`marmot/ffi.rs:384`). Its signature is `(handle, action_json) -> *mut c_char` and its body parses an "op envelope" JSON `Value` and routes it via `nmp_marmot::projection::ops::dispatch`. **That is a second, parallel, app-specific action-dispatch system running alongside the kernel's `nmp_app_dispatch_action` seam.** The entire `dispatch_action` architecture — the thing reviews #13–#36 fought to make real — exists so that there is *one* namespaced action entry point. Marmot has quietly built its own.

**The cost of leaving it undocumented:**
- It is invisible to the live/inert accounting. `nmp_app_chirp_marmot_dispatch` has many op types, none of which appear in any `dispatch_action` namespace census. The project cannot see its own action surface.
- It is a precedent. NIP-17's send executor (§2) needs signer access; the path of least resistance is "do what Marmot did — a bespoke `nmp_app_chirp_dm_*` cluster." If Marmot's exception is unnamed, that is not even a deviation, it is just "the other way we do it."
- It cannot be doctrine-linted. The `dispatch_action` seam has D0/D6/D7 invariants the linter checks. A bespoke `marmot_dispatch` envelope is outside that net.

**Recommendation: write `docs/decisions/0025-marmot-bespoke-ffi-cluster.md` this cycle.** It must do one of two things, explicitly: (a) declare the Marmot cluster a *permanent named exception* with a stated reason (MLS group state is stateful and handle-scoped in a way the stateless `dispatch_action` seam is not — that is a *defensible* argument), and a hard rule that it does not grow; or (b) declare it *transitional debt* with a migration target onto `dispatch_action`. Either is fine. Silence is not — silence is how a second architecture becomes the architecture. And the cluster *is* getting attention: PR #110-era work added `nmp_app_chirp_register_group_chat` right next to it. Name the boundary now, before NIP-17 blurs it.

---

## 4. The `nmp.zap` cliff

**`nmp.zap` is inert for the 5th consecutive review.** Verified on `origin/master`:
- `grep -rn CapabilityResultReady crates/ --include='*.rs'` → empty. ADR-0024 item 1: **not done.**
- `grep -rn deliver_capability_result` → empty. Item 2: **not done.**
- No `ActionModule::resume` (the two `resume_*` hits are the unrelated publish engine). Item 3: **not done.**
- Items 4 (Swift `URLSession` → `deliver_capability_result`) and 5 (`ZapModule` saga state machine) depend on 1–3: **not done.**

ADR-0024 is **0/5**, unchanged since #36.

**Is there a path to a working zap that skips ADR-0024? No.** The zap saga is: build kind:9734 → **LNURL GET** → **LNURL POST** → bolt11 → wallet pays. The two LNURL legs are multi-second HTTP calls. ADR-0024 exists precisely because blocking the single actor for seconds is a D3/D8 violation. There is no shortcut: any working zap requires non-blocking HTTP re-entry, which *is* ADR-0024. The current executor (`zap_request_command`, `nip57/action.rs:162`) publishes the kind:9734 to Nostr relays and stops — it produces a `correlation_id` and moves zero satoshis. A Swift "zap" button today would be shipping a lie.

**So the honest answer is the one #36 hinted at and #38 makes a hard recommendation: delete the registration.** Specifically, delete the `register_nip57_actions(unsafe { &mut *app })` call at `ffi.rs:175` and the `register_nip57_actions` function body at `ffi.rs:486-488` (and its imports at `ffi.rs:44`). Three lines plus an import. This does not delete `nmp-nip57` the crate — the kind:9734 builder, bolt11 decoder, and the security fix from PR #111 all stay, ready for the day ADR-0024 lands. It deletes the *registration* that makes `nmp.zap` dispatchable. The effect: the action surface census stops counting a payment feature that moves no money, and the next person to re-add `register_nip57_actions` must do it as part of a PR that also ships ADR-0024's checklist — which is exactly the gate ADR-0024 wrote for itself and `ZapModule` violated.

Keeping the registration "so it's ready" is the precise habit that produced 5 reviews of inert surfaces. Readiness lives in the crate. The registration is a claim, and right now the claim is false.

---

## 5. One thing to delete right now

**Delete the `register_nip57_actions` registration — `apps/chirp/nmp-app-chirp/src/ffi.rs:175` + `:486-488` + the `nmp_nip57::action` import at `:44`.**

The case, made plainly:
- **It is inert.** Zero Swift callers (`git grep zap origin/master -- '*.swift'` finds only `ChirpColor.zap` styling).
- **It cannot become live this cycle, or next.** ADR-0024 is 0/5 and is itself a multi-PR effort. Unlike `nip29.react_in_group` (one Swift call site away from the *existing* `GroupChatView`), `nmp.zap` has no near path.
- **It is actively misleading.** `nip29.react_in_group` registered-but-uncalled is a missing button. `nmp.zap` registered is a *dispatchable payment action that moves no money* — a strictly worse failure mode, because a caller gets a success-shaped `correlation_id`.
- **It violates its own ADR.** ADR-0024's checklist is captioned "Required before `ZapModule` can land." `ZapModule` landed anyway. Deleting the registration restores the gate the ADR wrote.
- **The cost of deletion is ~3 lines and reversible.** `nmp-nip57`'s builder/decoder/security logic is untouched and tested.

**Secondary finding (delete-eligible, lower priority):** `nmp.nip57.zaps` — the `ZapsDomain` `DomainModule` at `nip57/domain.rs:10` — is registered by no host (`grep ZapsDomain apps/ crates/nmp-core/` → empty). Same `DomainModule`-shell dormancy as `WelcomeUnwrapModule`. It is not the *one* delete (it is internal, lower blast radius) but it belongs on the same list: `DomainModule` impls that no host drives are trait shells, and the project keeps minting them.

**Explicitly NOT the delete: the 2 inert `nip29.*` executors.** `react_in_group` and `comment_in_group` are each one Swift call site away from the *already-merged, already-navigable* `GroupChatView`. Adding a long-press "react" on a `GroupChatMessageRow` consumes `nip29.react_in_group` in ~15 lines of Swift. They have a near path; `nmp.zap` does not. Wire them next cycle or delete them the cycle after — but `nmp.zap` is the one to cut *now*.

---

## 6. What NMP should NOT do

- **Should not let NIP-17 copy the Marmot bespoke-FFI pattern.** The signer-access seam NIP-17 needs (§2) must go through a kernel `ActorCommand`, not a `nmp_app_chirp_dm_dispatch` side-channel. If §3's ADR-0025 is written first, this is automatic.
- **Should not model NIP-17 inbound ingest as a `DomainModule`.** `WelcomeUnwrapModule` proves that `DomainModule` is a dormant trait shell. The live ingest pattern is `KernelEventObserver` + `register_snapshot_projection` — `GroupChatProjection` is the template. Copy it.
- **Should not write ADR-0025+ for new features.** The one ADR worth writing this cycle is `0025-marmot-bespoke-ffi-cluster.md` — a *boundary-naming* ADR for debt that already exists, not a decision record for new scope. ADR-0024 remains 0/5; no new feature ADR should precede closing it.
- **Should not re-register `nmp.zap` until ADR-0024's 5 items are done.** After §5's deletion, the gate is structural: re-adding `register_nip57_actions` forces the PR to also ship the async-capability protocol. That is the correct coupling.
- **Should not fan out parallel agents on NIP-17.** It is one coupled story: signer-access `ActorCommand` → kind:14 builder → inbound projection → 2 Swift screens. The 15 idle `worktree-agent-*` branches (all carrying only an old `MarmotGroupChatView.swift`) are evidence that fan-out here manufactures stale branches, not throughput.

---

## Appendix A — verification commands run

```
git rev-parse HEAD; git rev-parse origin/master      # HEAD 62ebbf1d is 1 behind origin (25d03390 = PR #115)
git show origin/master:ios/Chirp/Chirp/Features/GroupChatView.swift     # NIP-29 screen IS merged
git show origin/master:ios/Chirp/Chirp/Bridge/GroupChatBridge.swift     # postChatMessage → nip29.post_chat_message
git grep -n "react_in_group|comment_in_group" origin/master -- '*.swift'   # empty → both inert
git grep -n "GroupChatView" origin/master -- '*.swift'   # MarmotGroupsView.swift:98 → navigable
grep -rn "CapabilityResultReady|deliver_capability_result" crates/ --include='*.rs'   # empty → ADR-0024 0/5
grep -rln -i marmot docs/decisions/                  # empty → no Marmot ADR
grep -rn "ZapsDomain" apps/ crates/nmp-core/         # empty → nmp.nip57.zaps domain inert
ls docs/perf/codex-reviews/opus-review-37.md         # absent → #36 is the real predecessor
```

## Appendix B — register_nip29_actions, post-deletion

`register_nip29_actions` (`ffi.rs:452-456`) now contains exactly **3 `wire_action!` calls** — `post_chat_message`, `react_in_group`, `comment_in_group` — down from 15. `nmp-nip29/src/lib.rs` confirms the `domain` (13 `DomainModule` impls) and `view` (7 reactive views) modules were deleted as dead, zero-non-test-consumer code. The crate is now: 3 chat executors + `GroupChatProjection` + protocol primitives (`group_id`, `kinds`, `cache`, `interest`). This is exactly the shape #36 asked for.

---

## The single highest-leverage action

**Delete the `register_nip57_actions` registration (§5) this cycle, and write the Marmot-FFI boundary ADR (§3).** Together that is < 1 day of work, ships zero new inert surface, and closes the two remaining ways the project hides its own state from itself: a payment action that moves no money, and a second action-dispatch architecture nobody decided to have. With those closed, NIP-17 (§2) is a clean copy of the now-proven PR #115 pattern plus one honest new seam — and the project will, for the first time in the review series, have a consumer pipe with no dishonest entries in it.
