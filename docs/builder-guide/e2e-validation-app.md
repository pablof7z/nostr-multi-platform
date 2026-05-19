# E2E Validation App — "Pulse"

**Status:** spec for ONE builder-agent session. Companion: [`e2e-validation-build.md`](./e2e-validation-build.md) (the how).

**Scope priority (per orchestrator HB31 directive + user intent "run on my iPhone"):**
The user's stated goal — "empirical proof on real iOS hardware before M11 podcast rebuild" — makes device install (L5) a **required** rung, not a stretch goal. The L1–L5 ladder in §6 lists rungs in execution order, but the success criterion is **all rungs land**. A rung that fails (e.g. dev-cert issues block L5) is an **escalation to the user**, not "partial success." The fallback for an L5 blocker is to file an issue, fix it in this session, and re-attempt — not to declare done.

The ladder exists so that if mid-rung errors force a context-window stop, the durable state on master is coherent up to the last landed rung.

---

## 1. App identity + why this one

**Pulse** — a minimal Damus/Twitter-style timeline. Read kind:1 from one or more followed pubkeys, like (kind:7), reply, publish, follow/unfollow, switch between two accounts. Five SwiftUI screens. The smallest app shape that exercises every kernel subsystem landed M2–M8 + M10.5 + framework-magic.

Rejected alternatives:
- **Roll-call (NIP-29 groups)** would force NIP-29 wiring through FFI — that's a different rabbit hole (`crates/nmp-nip29` exists but no FFI surface). Too much new substrate.
- **Watermark (profile viewer)** wouldn't exercise publish (M7) or signing (M6). Half the surface.
- **Stress (extend NmpStress)** would conflate validation app with bench harness. NmpStress's purpose is performance, not protocol correctness. They are different deliverables.

Pulse exercises: M2 planner (timeline filter compiled to REQ), M3 EventStore (LMDB persistence across cold-restart), M4 NIP-77 (negentropy backfill triggered manually + on cold open), M5 NIP-42 (handshake against `nostr.wine`), M6 signers (Local nsec + NIP-46 bunker + `AccountManager`), M7 publish (kind:1 + kind:7 + kind:3-update via `Nip65OutboxResolver`), M8 RelayManager (real tungstenite websockets, connection pool, auth gate), M10.5 hardening (FFI surface stress under real load), framework-magic C7/C10/C11/C12/C13 (kind:3 auto-rewire visible in UI without app code).

---

## 2. Screens (5 total)

### Screen 1: Onboarding (`OnboardingView.swift`)
**Purpose.** First-launch entry. No data yet.
**Renders.** Two buttons: "Paste nsec" (modal text field) and "Connect bunker" (modal text field for `bunker://…` URI). Plus a "Create new" button that generates a fresh nsec, displays it once with a "I've saved it" checkbox, then proceeds.
**Dispatches.** `nmp_app_signin_nsec(nsec)` or `nmp_app_signin_bunker(uri)` (see build doc §2 for signatures).
**Observes.** `state.accounts.count > 0 && state.active_account != nil` → auto-navigate to Timeline.
**Errors.** `state.last_error_toast` if signin fails (invalid bech32, bunker timeout, etc). D6.

### Screen 2: Timeline (`TimelineView.swift`)
**Purpose.** Live feed of kind:1 from `active_account`'s follows, sorted by `created_at` desc.
**ViewSpec (kernel-side).** Open a `FollowingTimeline` view bound to active account. Planner compiles to one REQ per write-relay set (M2 + Nip65OutboxResolver). NIP-77 negentropy first on cold open per D2.
**Renders.** `List` of `NoteRow` (avatar, display name from profile cache, content, timestamp). Pull-to-refresh triggers `nmp_app_trigger_sync` on the current filter. Floating "compose" button → Screen 4. Tap a row → Screen 3.
**Diagnostics overlay (toggle via top-right gear).** Shows: rev counter, snapshot count/sec, relay status table, in-flight subs count, publish queue depth, last error toast. Mirrors what D6/D8 expose.

### Screen 3: NoteDetail (`NoteDetailView.swift`)
**Purpose.** One note expanded with reply thread + like button.
**ViewSpec.** `Thread { root_event_id }` — planner compiles to REQ for the event + `#e` replies (existing `OpenThread` ActorCommand). Plus `OpenAuthor { pubkey }` for the author header.
**Renders.** Top: full note. Middle: heart button (kind:7 react) + reply button. Bottom: replies list (kind:1 with `#e` referencing root).
**Dispatches.** `nmp_app_react(target_event_id, "❤")` and (via Screen 4 modal) `nmp_app_publish_note(content, reply_to_id)`.
**Observes.** Active subscription's snapshot. Like button toggles based on whether active_account already reacted (kind:7 from self with `#e` matching).

### Screen 4: Compose (`ComposeView.swift`)
**Purpose.** Author a kind:1, optionally as a reply.
**Renders.** `TextEditor` + "Send" button.
**Dispatches.** `nmp_app_publish_note(content, reply_to_id_or_null)`. The kernel resolves write-relays via `Nip65OutboxResolver` (M7 + D3) — no relay selection in the UI.
**Observes.** `state.publish_queue` to show "Publishing… → Sent to N relays / Failed on M" status before dismissing. Toast on failure (D6).

### Screen 5: Accounts (`AccountsView.swift`)
**Purpose.** Multi-session switcher + relay editor + follow editor.
**Renders.** Three sections.
- **Accounts.** List of `state.accounts` (display name + npub). Tap → `nmp_app_switch_active(id)`. "+ Add" → re-presents Screen 1.
- **Relays.** List of `state.relays` with status badge (connecting / connected / auth-required / auth-ok / error). "+ Add" prompts for URL + role (read / write / both). Trash icon → remove.
- **Follows.** Editable list of pubkeys from active account's latest kind:3. "+ Add" prompts for npub or hex. Tap-to-remove. Dispatch sends a fresh kind:3 (M7) which the kernel re-publishes via outbox; the kind:3 auto-rewire (framework-magic C8/C13) then re-opens timeline subs without app code.

---

## 3. Relay set (3 real relays — capabilities to verify, not assert)

| URL | Role | Expected capability | Why include |
|---|---|---|---|
| `wss://relay.damus.io` | default | strfry → NIP-77 likely, NIP-42 optional | Vanilla high-availability fallback. Likely to "just work" cold. |
| `wss://nos.lol` | default | strfry → NIP-77 likely | Backup for damus.io. Vanilla NIP-01. |
| `wss://nostr.wine` | opt-in (Accounts → +Relay) | NIP-42 required, paid pubkey allowlist | Proves M5 NIP-42 handshake against a real relay. **Note: writes will fail unless the test nsec is whitelisted; this is expected and the M5 evidence is the handshake itself completing, observable in the relay-status badge.** |

The capability column says "expected" / "likely" — `Nip77CapabilityProbe` (`crates/nmp-nip77/src/capability.rs`) is the source of truth at runtime. If `nostr.wine` no longer enforces NIP-42, the smoke test (§5) catches that and the user just won't see an `auth-required` badge. That's fine; M5 still has unit-test coverage.

**Read-discovery fallback** for `Nip65OutboxResolver` (when a recipient's read-relays for `#p` routing are unknown): `[wss://relay.damus.io, wss://nos.lol]` — hard-coded constant in the resolver's constructor for now (proper indexer story is post-v1). **Crucially, this fallback is read-side discovery only.** Per D3 (outbox automatic), publishing requires the active account to have declared its own write-relays (kind:10002). If the active account has no kind:10002, `nmp_app_publish_note` MUST surface a `last_error_toast` ("active account has no write-relays declared — add a relay in Accounts → Relays") rather than silently posting to undeclared relays. The Accounts screen exposes "Publish my kind:10002" as the bootstrap path.

---

## 4. Diagnostics overlay (D6 + D8 observability)

The Timeline's gear-icon toggle reveals a fixed pane at the bottom:

```
rev: 14721 | snap/s: 8 | subs: 3 active | pub_q: 0
relays:
  wss://relay.damus.io    connected   1240 events
  wss://nos.lol           connected     312 events
  wss://nostr.wine        auth-ok        14 events
last error: (none)
```

These fields read directly off the `state` JSON the kernel emits. No platform-side derivation. If the kernel doesn't surface a counter, that's a kernel gap to file, not a Swift-side fix.

---

## 5. Real-relay smoke test (Rust companion)

`crates/nmp-testing/tests/real_relay_smoke.rs` — `#[ignore]` by default. Run with:

```bash
cargo test -p nmp-testing --features test-support \
  --test real_relay_smoke -- --ignored --nocapture
```

Scenarios (each `#[test] #[ignore]`):

1. **`damus_round_trip_kind1`** — generate fresh keys, connect to `wss://relay.damus.io`, publish a kind:1 with random nonce in content, REQ it back by id+author within 5s, assert content matches. **Proves M7+M8+M3 over a real socket.**
2. **`damus_follow_then_kind3_rewire`** — same keys, publish kind:3 with one follow, open `FollowingTimeline`, assert the followed pubkey's recent kind:1 (if any) lands in snapshot within 10s. Then publish a new kind:3 adding a second follow; assert a new REQ goes out for the new pubkey **without re-creating the view handle** (framework-magic C8/C13). **Proves M2 + framework-magic.**
3. **`nip77_backfill_on_cold_open`** — using a pre-seeded local LMDB store with one event from author A, connect to a relay known to have more events from A, open a timeline, verify NIP-77 reconciliation runs (counters non-zero) and missing events arrive before EOSE (or count savings vs REQ baseline). **Proves M4 + D2.** Falls back to a soft assertion (capability cache marks unsupported) if the relay rejects negentropy frames.
4. **`nostr_wine_nip42_handshake`** — connect to `wss://nostr.wine`, attempt a REQ, expect AUTH challenge, sign and send AUTH, assert subsequent REQ returns events (or returns CLOSED with `auth-required:` removed). **Proves M5.** Skipped (not failed) if the relay isn't reachable.
5. **`outbox_resolves_to_kind10002_writes`** — pre-publish a kind:10002 from test keys listing one write-relay (e.g. `wss://nos.lol`), then `PublishTarget::Auto` on a fresh kind:1, assert the publish lands ONLY on nos.lol (verify by REQ-ing back from nos.lol vs damus.io). **Proves `Nip65OutboxResolver` + D3.**
6. **`multi_session_switch_replans`** — instantiate `AccountManager` with two test signers, install `ActiveAccountReactor` against an in-process kernel, switch active, observe that subscriptions for account-A close and account-B's open within a deterministic tick budget (≤200ms). **Proves M6 + ActiveAccountReactor.** This one can be substantially mock-backed (use `MockRelay` from `relay-builder`) since it tests internal reactor wiring, not socket I/O.

The Rust smoke test is the **authoritative validation**; the iOS app is the user-facing exercise of the same code paths. If smoke passes and the app fails, the bug is in Swift/FFI glue.

---

## 6. Rung ladder (execution order — all rungs REQUIRED)

L1–L5 are the **build order**, not a "ship what you can" gate. All five rungs must land. An L5 blocker (dev-cert friction, devicectl quirks) is an escalation back to the user — not a green light to declare done at L4.

| L | Scope | Subsystems proven | Demo |
|---|---|---|---|
| **L1** | Onboarding (nsec-paste only) + Timeline reading from 1 followed pubkey on `relay.damus.io` in **simulator** | M2 + M3 + M6 (local signer only) + M8 | "Paste nsec, see one followed user's recent notes." |
| **L2** | + Compose (`publish_note`) + `Nip65OutboxResolver` lands → kind:1 round-trip visible in own Timeline | M7 + D3 | "Compose 'hello pulse', see it appear in own timeline within 5s." |
| **L3** | + Accounts screen + add 2nd account (nsec or bunker) + `switch_active` + `ActiveAccountReactor` lands → subscriptions re-plan | M6 multi-account + `ActiveAccountReactor` | "Switch account, see different timeline within 1s." |
| **L4** | + Follow-edit (publish new kind:3) + kind:3 auto-rewire makes new follow's notes appear in timeline without app code + NoteDetail screen (Screen 3) with reply + react | Framework-magic C8/C13 + kind:7 | "Add follow, see their notes land. Tap a note, see replies + heart it." |
| **L5** | + Real iPhone install via Xcode + dev-cert | Real device | "Run on user's plugged-in iPhone, repeat L2 + L3 demos." |

Beyond L5 (sequence permitting before context ends): NIP-42 add-relay (`nostr.wine`), full diagnostics overlay polish, NIP-77 manual trigger button + counters in overlay, screenshot capture script.

**NoteDetail clarification.** Screen 3 (NoteDetail with replies + likes) is part of the 5-screen scope and lands in L4. The "stretch beyond L5" reference is to *polish* of the overlay/trigger UI, not the screen itself.

---

## 7. Demo script (manual walkthrough — proves each subsystem)

The QA agent (or user) runs these in order against the simulator first, then the iPhone. Each step lists what to do, what to expect, which subsystem it proves, what to capture.

1. **Cold launch (sim).** Tap `NmpPulse.app`. Expect Onboarding screen within 2s. **Proves**: FFI bootstrap, actor thread starts. **Capture**: launch-to-render-ms via `xcrun simctl spawn booted log show --predicate 'subsystem == "com.nmp.pulse"' --last 1m`.
2. **Paste nsec.** Use the pre-baked test nsec from `crates/nmp-testing/fixtures/test_nsec.txt` (see build doc §6 — builder generates this). Tap "Sign in". Expect → Timeline screen within 1s, empty list. **Proves**: M6 LocalSecretKeySigner. **Capture**: `state.accounts.count == 1`, `state.active_account == <id>`.
3. **Wait for cold backfill.** Within 10s, expect at least 5 kind:1 rows to appear. **Proves**: M2 planner → M8 socket → M3 insert → snapshot → SwiftUI update. **Capture**: time-to-first-row, time-to-five-rows, `state.rev` increments.
4. **Open relay diagnostics.** Tap gear icon. Expect: relay table shows `relay.damus.io` and `nos.lol` both `connected`, event counters > 0. **Proves**: D8 observability + D7 capability cache populated. **Capture**: screenshot of overlay.
5. **Compose note.** Tap compose, type `pulse e2e <timestamp>`, tap Send. Within 5s expect the note to appear in own Timeline. **Proves**: M7 + `Nip65OutboxResolver` + D3 (no relay picker in UI). **Capture**: `state.publish_queue` transitions empty → 1 → 0; screenshot of own note in timeline.
6. **Add follow.** Accounts → Follows → "+ Add" → paste pubkey of a known prolific user. Within 10s expect their recent notes to appear in Timeline. **Proves**: kind:3 publish + auto-rewire (framework-magic C8/C13) + planner re-compile. **Capture**: REQ frames in relay-status diagnostics show new pubkey in filter; `state.rev` advances; new rows appear with timestamps from before the follow was added (proves backfill ran, not just live).
7. **Add 2nd account.** Accounts → "+ Add account" → paste a 2nd nsec (or bunker URI). Switch active. Expect Timeline to repopulate from account-B's follows within 2s. **Proves**: `AccountManager` + `ActiveAccountReactor`. **Capture**: snapshot before/after switch; subs count in diagnostics resets.
8. **Add auth-required relay.** Accounts → Relays → "+ Add" → `wss://nostr.wine`, role: read. Expect relay-status badge to transit `connecting → auth-required → auth-ok` (or stay `auth-required` if the test nsec isn't allowlisted — that's still M5 proof). **Proves**: M5 NIP-42 handshake. **Capture**: diagnostics screenshot showing the transition.
9. **Force NIP-77 sync.** Pull-to-refresh on Timeline. Expect a brief "syncing" state in diagnostics, neg counters increment (`state.diagnostics.nip77_bytes_in`). **Proves**: M4. If the configured relays don't support NIP-77, capability cache marks them unsupported, and the trigger falls back silently to REQ — that's also correct behavior per D2. **Capture**: counters before/after.
10. **Kill + cold-restart.** Stop the app via `xcrun simctl terminate booted com.nmp.pulse`. Re-launch. Expect: skip Onboarding (signers persisted via Keychain wrapper), Timeline populates from LMDB cache within 2s, then live-updates from relays. **Proves**: M3 LMDB persistence + M6 signer persistence. **Capture**: cold-restart-to-first-row time; LMDB file on disk via `xcrun simctl get_app_container booted com.nmp.pulse data` then `ls -la Library/Application\ Support/`.

Repeat steps 2, 3, 5 on the physical iPhone after Xcode `Run on Device`. The other steps are simulator-sufficient.

---

## 8. Known risks + fallbacks

| Risk | Mitigation |
|---|---|
| iOS dev-cert friction → can't install on device | L1–L4 on simulator are still complete success. L5 is explicitly stretch. |
| Test nsec gets rate-limited / banned on real relays | Pre-baked nsec is a fresh one; if banned, generate a new one in onboarding's "Create new" path. |
| LMDB path on iOS sandbox | Use `FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first!.appendingPathComponent("nmp")`. Pass as C string into a new `nmp_app_set_storage_path(app, path)` FFI call BEFORE `nmp_app_start`. Skipped: LMDB-on-iOS may surface heed crate iOS issues — fallback to MemEventStore (current default) if `lmdb-backend` feature breaks the build. Document the gap. |
| NIP-46 bunker timeout in onboarding | Show toast after 30s, allow user to back out and try nsec. Bunker is L3 stretch; if it doesn't work in this session, file an issue and ship L1–L2 + L3 with nsec-only multi-account. |
| `nostr.wine` paid-relay write rejection | Expected. The proof is the handshake completing, not the write succeeding. |
| `relay.damus.io` outage during demo | The app has nos.lol as a second default; reading should still work. If both are out, the smoke test will catch it earlier. |

---

## 9. Out of scope (deferred — do NOT build)

- **Audio playback / podcast features** — M11 has its own iOS app track (`ios/NmpPodcast`); not relevant for kernel validation.
- **DMs (NIP-17 / NIP-44 / NIP-59)** — deferred to post-v1 per `docs/plan/scope-adjustments-2026-05-18.md`.
- **Wallet (NIP-47 / NIP-57 / NIP-60 / NIP-61)** — deferred to post-v1.
- **WoT scoring (M13)** — not started.
- **Blossom uploads (M10)** — separate milestone.
- **NIP-29 groups (M11.5 Highlighter app's domain)** — separate iOS track.
- **Push notifications, App Store submission, deep-linking, NSE background decryption** — productionization, not framework validation.
- **UniFFI codegen path (M14)** — explicitly using raw C FFI (Path A) per build doc §3. M14 will supersede this app's bridge.
- **UI polish, animations, accessibility, dark-mode tuning** — diagnostic-grade UI is enough for validation.

---

## 10. Handoff to builder

Read this file, then [`e2e-validation-build.md`](./e2e-validation-build.md). Build in L-ladder order — L1 first, fully working in simulator, before touching L2. Commit per ladder rung. Push via `git push origin HEAD:master`. Post-merge codex review per `~/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/memory/post-merge-codex-review.md`.

QA agent (next dispatch) will run §7 against simulator first, then ask the user to plug in their iPhone for steps 2/3/5.
