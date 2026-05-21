# Opus Direction Review #37 — Group chat already shipped. It just isn't NIP-29.

Date: 2026-05-21
Reviewer: Opus (architectural direction audit)
Scope: NMP kernel + `dispatch_action` seam + Chirp FFI + NIP-29 (15 executors / 7 views / 13 domain modules) + the read-side projection model + ADR-0024. Ground truth: `git log master`, `gh pr list`, file-reads at named line numbers.

---

## TL;DR — read this first

- **The single load-bearing finding: Chirp already ships a working group-chat screen — `MarmotGroupChatView.swift` (256 LoC, on master since 2026-05-20) — and it has nothing to do with NIP-29.** It is MLS-encrypted Marmot groups, served by a *separate, bespoke FFI cluster* (`nmp_app_chirp_marmot_register`, `nmp_app_chirp_marmot_group_messages`, `nmp_app_chirp_marmot_dispatch` in `apps/chirp/nmp-app-chirp/src/marmot/ffi.rs`). It does **not** route through `dispatch_action`, does **not** use the snapshot-projection seam, and does **not** touch any of the 15 NIP-29 executors or 7 NIP-29 views.
- **Review #36's central plan — "build one NIP-29 group-chat screen as the forcing function for the 15 dormant executors" — has been overtaken by events.** The group-chat *use case* got built. It got built on a third parallel seam. This is the exact "seam built, consumers route around it" pathology reviews #19, #26, and #36 named — now reproduced one layer up: the project has a *generic action seam* (`dispatch_action`), a *generic projection seam* (`register_snapshot_projection`), and a *third bespoke Marmot FFI cluster* that uses neither.
- **The honest live/inert count did not improve this cycle and the trajectory got worse, not better.** Live user-reaching `dispatch_action` calls: still **4** (`nmp.publish`, `chirp.react`, `chirp.follow`, `chirp.unfollow` — `KernelBridge.swift:191/259/265/269`). Inert surfaces: `nmp.zap`, 15× `nip29.*`, `HttpCapability`, ADR-0024 — **unchanged at 6 from #36**. The substrate seam's customer base grew by zero. A *sibling* seam (Marmot) absorbed the one piece of demand #36 had earmarked as the seam's next customer.
- **PR #108 (`feat(nip29): add GroupChatProjection`) is open and is honest, scoped, read-side-only work — but it is building the read side of a screen that already exists via Marmot.** Its own body says the wiring closure and Swift view are "out of scope." It is a projection with no consumer, being added next to a use case already served by a different code path. Do not merge it as-is; see §1.
- **Highest-leverage next move is NOT another NIP-29 cycle. It is to DELETE `nmp-nip29`'s dormant read+domain layers (≈1.0K LoC, zero non-test consumers), make a hard product call on whether relay-based NIP-29 groups are even on the roadmap given Marmot ships MLS groups, and pick NIP-17 DMs as the next genuinely-missing capability.**

---

## The shape of the problem — it changed

Reviews #25–#36 diagnosed "shipped-but-inert": registration side built, consumption side absent. #37 finds the *next* stage of that disease, and it is more expensive:

**The project now builds a generic seam, finds it inconvenient, and ships the actual feature on a bespoke parallel seam — leaving the generic seam inert AND a pile of dead protocol code behind it.**

Concretely:

| Capability | Generic seam it was supposed to use | What actually shipped | Generic seam status |
|---|---|---|---|
| Post a note | `dispatch_action` `nmp.publish` | `KernelBridge.publishNote` → `dispatch_action` | **Live — correct** |
| React / follow | `dispatch_action` `chirp.*` | Swift callers → `dispatch_action` | **Live — correct** |
| Group chat | `dispatch_action` `nip29.*` + projection seam | `MarmotGroupChatView` → bespoke `nmp_app_chirp_marmot_*` FFI | **Inert — bypassed** |
| Zap | `dispatch_action` `nmp.zap` + `HttpCapability` | nothing user-reaching | **Inert** |

The Marmot bypass is not wrong *as a product decision* — MLS groups are a legitimate, arguably better, group-chat substrate than NIP-29 relay groups. The problem is that the project spent ~3015 LoC building `nmp-nip29` (15 executors, 7 views, 13 domain modules) for a use case it then served a different way, and **no review caught the redundancy because the NIP-29 code compiles green and has tests.** Green CI on dead code is the camouflage, exactly as #33 warned.

---

## 1. Consumer-pipe check — the honest count

**Live `dispatch_action` call sites (verified in `ios/Chirp/Chirp/Bridge/KernelBridge.swift`):**
- `nmp.publish` — `publishNote`, line 191
- `chirp.react` — `react`, line 259
- `chirp.follow` — line 265
- `chirp.unfollow` — line 269

That is **4**, identical to #36.

**Inert registered surfaces (verified):**
- `nmp.zap` — registered in `ffi.rs:434`; zero Swift callers; executor publishes kind:9734 then stops (`nmp-nip57/src/action.rs` — the LNURL leg is a doc comment at lines 23/36/65, not code).
- 15× `nip29.*` — registered in `ffi.rs:383-403`; the *only* references to a `"nip29.*"` namespace string anywhere outside the `nmp-nip29` crate are three lines in `ffi.rs`'s own `#[cfg(test)]` module (lines 577/586/594). Zero production callers. Zero Swift callers.
- `HttpCapability` — `HttpCapabilityWiring` is referenced only in `nmp-core/src/substrate/{mod,http}.rs` (its own definition) and a doc comment in `nmp-nip57/src/action.rs`. Zero executors invoke it.
- ADR-0024 async capability — still 0/5 checklist items (`grep` for `CapabilityResultReady` / `deliver_capability_result` / `fn resume`: 0 hits).

**Trajectory.** #36 said "6 inert, 4 live." #37 says the same 6:4 — but with a worse subtext. #36 believed the next cycle would convert one `nip29.*` executor to live via a screen. Instead the screen got built on Marmot and PR #108 adds *more* dormant NIP-29 code (`GroupChatProjection`) on top of the 15 dormant executors. The pipe is not just un-improved; the inert pile is actively still growing.

**On PR #108 specifically:** it is well-engineered and honestly scoped (read-side `KernelEventObserver`, dedup by id, D6 degrade-to-empty). But it is the read side of a NIP-29 group-chat screen, and Chirp's group-chat screen is Marmot. Merging #108 adds a 5th dormant NIP-29 artifact. **Recommendation: do not merge #108 until §3's product call is made.** If relay-based NIP-29 groups are killed, #108 is dead-on-arrival. If they are kept, #108 needs a committed Swift consumer in the *same cycle* — not "a follow-up."

---

## 2. The projection model — right or wrong?

The brief asks about "a trait that serializes event-store state as JSON at query time." **That trait does not exist** — PR #108's author found the same thing and says so in the PR body. The real read-side model is:

- A `KernelEventObserver` (e.g. `ModularTimelineProjection`, `nmp-nip01/src/timeline_projection.rs:60`) holds `Mutex<Inner>` state, mutated on every `on_kernel_event`.
- `snapshot()` clones the whole state and serializes it to JSON on demand.
- The FFI (`nmp_app_chirp_snapshot`, `ffi.rs:214`) calls `snapshot()` → `serde_json::to_string` → fresh `CString` on **every** Swift pull.

**Is this tenable for 500-message group chat?** The ceiling is real but it is not where the brief thinks:

- **The per-insert cost is the worse offender, not the per-snapshot cost.** `EventAccumulator::insert` (`nmp-nip29/src/view/shared.rs:21-29`) does a full `Vec::sort_by_key` on **every single event insert** — O(N log N) per event, O(N² log N) to build a room from cold. For 500 messages that is ~4500 comparisons per new message. PR #108's `GroupChatProjection` uses a `BTreeMap` keyed by id instead, which is the correct fix — so the *new* code already dodges this. But the 7 existing NIP-29 views in `view/shared.rs` still carry the O(N log N)-per-insert bug.
- **The per-snapshot cost** — clone the whole `Vec<KernelEvent>` + re-serialize — is wasteful (the `EventAccumulatorDelta` diff types exist at `shared.rs:56` but **no FFI carries a delta**; every pull is a full snapshot). For 500 small messages this is sub-millisecond JSON; tolerable. It becomes a real problem at thousands of messages or with rich content trees, but that is not the current ceiling.

**The honest verdict:** the snapshot-on-every-tick model is *fine for now* and the architecture is not the bottleneck. **But it is moot for the shipping product** — Chirp's group chat is Marmot, which reads MLS state through `nmp_app_chirp_marmot_group_messages`, an entirely separate path. So: do not invest in a delta-based projection FFI as a priority. It is real debt; it is not a live user-facing problem; and the read model it would optimize (`nmp-nip29` views) has zero consumers. **The right fix is deletion, not optimization** (§3, §6).

---

## 3. The 15 dormant NIP-29 executors — keep or delete? Named.

#36 set a 2-cycle clock and said "build one screen, delete what it doesn't reach." The screen got built — as Marmot. **The clock has effectively expired. #37's verdict is delete almost all of it, and it is more aggressive than #36's because the "plausibly reachable from a chat UI" rationale collapsed when chat went to Marmot.**

**Delete now — no plausible consumer, no roadmap, served by Marmot or by nothing (12 executors):**
- `nip29.create_group`, `nip29.edit_metadata`, `nip29.put_user`, `nip29.remove_user`, `nip29.create_invite`, `nip29.delete_event`, `nip29.delete_group` — the admin cluster (`action/admin.rs`, 195 LoC). Marmot has its own invite/membership; relay-group admin has no UI and no roadmap.
- `nip29.join_request`, `nip29.leave_request` — membership (`action/membership.rs`, 157 LoC). Same.
- `nip29.post_artifact`, `nip29.post_discussion` — content (`action/content.rs`). No artifact/discussion screen exists or is planned.
- `nip29.share_event_into_group` — composed (`action/composed.rs`). No caller.

**Keep on a 1-cycle clock — ONLY if §3's product call says relay-based NIP-29 groups ship (3 executors):**
- `nip29.post_chat_message`, `nip29.react_in_group`, `nip29.comment_in_group` — the only three a hypothetical NIP-29 (not Marmot) chat screen would consume.

**The decision that gates all of the above — make it explicitly, this cycle:** *Does NMP ship relay-based NIP-29 groups at all, given Marmot/MLS already ships encrypted groups in Chirp?* Two coherent answers:
- **(a) No — Marmot is the group story.** Then delete all 15 executors, all 7 views, all 13 domain modules, and close PR #108. `nmp-nip29` shrinks to `group_id` + `kinds` (the parsing primitives, ~300 LoC) or is deleted outright. This is the recommended answer — it removes ≈2.7K LoC of dead protocol code and a whole class of "which group system?" ambiguity.
- **(b) Yes — NIP-29 public relay groups are a distinct product from Marmot private groups.** Then PR #108 must ship *with* a Swift `GroupChatView` consuming `nip29.post_chat_message` in the same cycle, and the 12 admin/membership executors above still get deleted (no admin UI is planned regardless).

There is no coherent third answer where 15 executors + 7 views + 13 domain modules sit dormant for a 6th review. The lukewarm "keep them, they're cheap" has now survived ~6 reviews; #37 says the cost was never the executors, it was the *ambiguity they create about what the product is*.

---

## 4. ADR-0024 sequencing — the brief's plan has lost its first step

The brief's plan: "NIP-29 screen first, then ADR-0024 NIP-05 verification as first async consumer." **The first half no longer has a concrete next step** — the NIP-29 screen went to Marmot, and "build a NIP-29 screen anyway" is now contingent on §3's unresolved product call.

So the sequencing inverts by default:

- **ADR-0024's first consumer should be NIP-05 verification, and it should be the next consumer-feature outright** — not because async is suddenly the priority, but because the NIP-29-screen-first half of the brief's plan is now blocked on a product decision that should be made deliberately, not used as a sequencing dependency.
- NIP-05 verification is the right *first* async consumer for the reason #36 already gave: single-hop (`GET /.well-known/nostr.json`), no saga state machine, produces a visible result (verified badge). It exercises all 5 ADR-0024 checklist items against the simplest possible shape.
- The brief's sub-question — "GroupChatProjection can't load relay data, only the local event store; how does that interact with reading group messages?" — **is moot.** PR #108 explicitly documents that a `KernelEvent` carries no relay provenance and the projection trusts the `relay_pin` routing lane to have already fetched the events. The projection is a pure local-event-store reducer *by design*; the network fetch is the planner/`relay_pin` lane's job (`view/chat.rs:32`, `ViewDependencies.relay_pin`). There is no async-capability interaction here — relay subscription is a different mechanism (`PushInterest` / the M2 compiler) from HTTP capability. The brief conflated two unrelated transports.

**Net:** ADR-0024 is still the right design and still 0/5 done. Its first consumer is NIP-05. But ADR-0024 is *not* the highest-leverage next thing — see §5.

---

## 5. The biggest missing capability — NIP-17 private DMs

Pick **NIP-17 (gift-wrapped private direct messages)**. Argument:

1. **Every serious Nostr client has 1:1 DMs.** A social client (Chirp) without DMs is conspicuously incomplete in a way that "no zaps yet" or "no NIP-29 public groups" is not. It is the single most-noticed absence.
2. **The substrate is already partly built.** `nmp-nip59` exists (gift-wrap / seal — the NIP-17 envelope), and per MEMORY's "D0 structural violations" entry it has a `WelcomeWrapModule` already wired for Marmot's NIP-59 Welcome delivery. The hard cryptographic primitive (NIP-44 encryption + gift-wrap) is done. NIP-17 is mostly *composition* of existing pieces, not new crypto.
3. **It is a clean single-screen shape** — a DM thread is structurally a `KernelEventObserver` projection (kind:14 rumor inside kind:1059 wrap) + one `dispatch_action` send verb. It maps perfectly onto the *working* seam (`dispatch_action` + projection), so building it would also be a genuine 5th customer of the generic seam — directly attacking the consumer-pipe pathology.
4. **It does not collide with Marmot.** Marmot is *group* (N-party MLS); NIP-17 is *1:1* (gift-wrapped). They are complementary, not duplicative — unlike NIP-29, which *does* collide with Marmot. Building DMs adds a capability without re-opening "which system owns groups?"

NIP-05 verification (the ADR-0024 consumer) is a defensible second pick, but it is an *enrichment* (a badge on a profile), not a primary capability. NIP-65 relay management already substantially exists (`AddRelay`/`RemoveRelay` actor commands, relay-edit rows, `RelaySettingsView.swift`). DMs are the one *primary, user-facing, entirely-absent* capability.

**Recommended next-feature ordering:** (1) §3 product call + the deletion it implies; (2) NIP-17 DM thread — read projection + one send verb, on the `dispatch_action`/projection seam; (3) ADR-0024 item 1 + NIP-05 verification as its first consumer.

---

## 6. What NMP should DELETE — `nmp-nip29/src/view/` and `nmp-nip29/src/domain/`

**Delete `crates/nmp-nip29/src/view/` (7 view types, 456 LoC) and `crates/nmp-nip29/src/domain/` (13 `DomainModule` impls, 379 LoC) in their entirety.**

Not because they are buggy — `view/chat.rs`'s `GroupChatView` is clean, correctly host-pinned code. Delete them because they are **the wrong abstraction for a problem the project decided to solve another way**:

- **Zero non-test consumers.** `grep` for `nmp_nip29::domain` / `nmp_nip29::view` across `crates/`, `apps/`, `ios/` outside the crate itself: nothing. The 7 views and 13 domain modules are reached only by `nmp-nip29`'s own tests.
- **The use case they model — group event projection — is served by Marmot** through `nmp_marmot::projection`, a completely different and *actually-shipping* read model.
- **They are pure read-side code with no callers**, which makes them the cleanest possible deletion: no FFI symbol to retire, no Swift to rewire, no actor command to drop. `git rm -r crates/nmp-nip29/src/{view,domain}` + delete the `pub mod` lines in `lib.rs:35-40` + the re-exports. ≈835 LoC gone with near-zero blast radius.
- PR #108's `GroupChatProjection` (`projection/mod.rs`) is the *only* read-side NIP-29 code with even a hypothetical future consumer — and even that is contingent on §3. The `view/` directory is strictly redundant with it: two read-side models for the same events, both dormant.

This is the highest-confidence deletion in the codebase. The more aggressive cut — delete the 15 executors and `nmp-nip29` entirely — is the *right* call too, but it is gated on §3's product decision. `view/` + `domain/` can be deleted **today, unconditionally**, because no answer to §3 keeps them: answer (a) deletes all of `nmp-nip29`; answer (b) keeps the *executors* and PR #108's projection but still has no use for the 7 `view/` types or the 13 `domain/` modules.

A secondary deletion candidate, lower confidence: the `nmp.zap` registration in `ffi.rs:434` and `ZapModule` — it has been inert for 3 reviews and shipping a zap button today moves zero satoshis. But zaps have a credible roadmap (ADR-0024 → LNURL leg), so this is "delete if not consumed within 2 cycles," not "delete now." The `view/`+`domain/` cut has no such roadmap.

---

## Stop / Start / Continue

### STOP
- **Stop adding NIP-29 code of any kind until §3's product call is made.** PR #108 included — do not merge it, do not build on it, until "do relay-based NIP-29 groups ship?" has an explicit yes/no.
- **Stop letting bespoke per-app FFI clusters absorb demand the generic seam was built for.** The Marmot FFI cluster (`nmp_app_chirp_marmot_*`) bypasses both `dispatch_action` and the projection seam. That may be acceptable for MLS (its state model genuinely differs), but it must be a *named, justified exception* in an ADR — not a silent third path. Otherwise every future feature will quietly grow its own FFI and the generic seam stays inert forever.
- **Stop counting `nmp-nip29` test coverage as evidence of value.** 3015 LoC, ~55 passing tests, zero non-test consumers. Green CI on dead code is the camouflage.

### START
- **Start with the §3 product decision.** One paragraph: "NIP-29 relay groups: ship / do not ship, given Marmot." Everything else (delete 12 executors, close or finish #108, delete `view/`+`domain/`) follows mechanically.
- **Start NIP-17 DMs as the next genuine capability** — on the working `dispatch_action` + projection seam, making it the 5th real consumer.
- **Start an ADR for the Marmot-FFI exception** if MLS keeps its bespoke cluster — so the next reviewer can tell an intentional exception from seam-erosion.

### CONTINUE
- The single-actor kernel boundary and the 4 live `dispatch_action` namespaces. Still the project's real, proven asset.
- D0/D6/D7/D8 doctrine-lint. The kernel architecture is clean; PR #108's D6 degrade-to-empty discipline is exactly right.
- The `relay_pin` routing lane and the principle that projections are pure local-event-store reducers (the network fetch is the planner's job) — that separation is correct and #108 documents it well.

---

## Appendix A — verification commands run

```
gh pr list --state open --limit 50
  → #108 feat(nip29): add GroupChatProjection ; #109 fix(nip29) admin tag fields
git log --oneline -20                            # PRs #99-#107 on master
grep -rn '"nip29\.' crates/ apps/ ios/ --include=*.rs --include=*.swift
  → only apps/chirp/.../ffi.rs lines 577/586/594, all inside #[cfg(test)]
grep -rln "HttpCapabilityWiring" crates/         # only substrate/{mod,http}.rs + nip57 doc comment
grep -rln "dispatchAction|dispatch_action" ios/Chirp --include=*.swift
  → KernelBridge.swift only: nmp.publish, chirp.react/follow/unfollow
ls ios/Chirp/Chirp/Features/                     # MarmotGroupChatView.swift (256 LoC, 2026-05-20)
grep -n "no_mangle" apps/chirp/nmp-app-chirp/src/marmot/ffi.rs
  → nmp_app_chirp_marmot_{register,group_messages,dispatch,...} — bespoke cluster
wc -l crates/nmp-nip29/src/{view,domain,action}/*.rs   # view 456 / domain 379 / action 989
```

## Appendix B — what the brief got wrong (stated honestly)

- "A `GroupChatProjection` is in progress (in a worktree PR, not merged)." — True: PR #108. But the brief implies it is *the* NIP-29 group-chat screen's read side. It is the read side of a screen that **already exists via Marmot on a different code path**. The brief did not know group chat had already shipped.
- "NMP's read side uses a Rust struct that serializes event-store state as JSON at query time" via "a trait." — No such trait exists. The model is `KernelEventObserver` + interior-`Mutex` + on-demand `snapshot()`. PR #108's author hit the same correction.
- "Building the async machinery first unlocks reading group messages because GroupChatProjection can't load relay data." — Conflates two transports. Relay subscription (`PushInterest` / M2 compiler / `relay_pin`) and HTTP capability (`HttpCapability` / ADR-0024) are unrelated. The projection reading only the local event store is *by design*; the network fetch is the planner's job. There is no async-capability dependency for reading group messages.

---

## The single highest-leverage action

**Make the §3 product call — does NMP ship relay-based NIP-29 groups at all, given Marmot already ships MLS groups in Chirp — and on a "no," delete `nmp-nip29`'s `view/` + `domain/` + the 12 admin/membership/artifact executors (≈2.5K LoC of dead protocol code) and close PR #108.** Then build NIP-17 DMs on the working `dispatch_action` + projection seam as the next genuine capability. The project's disease is not missing infrastructure — it is infrastructure built for use cases the project then served another way. The cure is a deletion and a decision, not a 25th ADR.
