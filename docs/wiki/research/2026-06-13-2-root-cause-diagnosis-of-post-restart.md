---
type: research-record
date: 2026-06-13
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
source_lines: 4612-4698
agent_attribution: a0bfdad69034b526e
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Root-cause diagnosis of post-restart live message reception failure in nmp-marmot, HYPOTHESIS CONFIRMED: register_with_keys never re-subscribes per-group kind:445 feeds"
captured_at: 2026-06-13T18:50:15Z
---

Root-cause diagnosis of post-restart live message reception failure in nmp-marmot, HYPOTHESIS CONFIRMED: register_with_keys never re-subscribes per-group kind:445 feeds

---

User: [Agent task result: Agent "Diagnose post-restart live msg gap" completed]
Confirmed. `create_group` (groups.rs:1681) persists relays via `replace_group_relays` into the SQLite `group_relays` table, and on the receive side `process_welcome`/accept (groups.rs:2245) also persists them. `get_relays` (groups.rs:1451 → 1454) reads them back. So group relays **survive restart in the MDK store** and are recoverable — the fix is sound.

I have a complete, code-grounded diagnosis. Here is my deliverable.

---

# Diagnosis: post-restart Marmot group messages never received

## Root cause — HYPOTHESIS CONFIRMED

On restart, `register_with_keys` rebuilds the `MarmotProjection` and re-registers the gift-wrap inbox interest, the ingest parser, observer, and push projections — but it **never re-pushes the per-group kind:445 message subscriptions for already-joined groups.** The kernel therefore keeps no live REQ for the group's relays, the raw-event tap never sees B's new kind:445 events, MDK never decrypts them, and the messages projection never updates. History renders because it's read from the persisted MDK SQLite store; live receive is dead because the subscription was never re-installed.

### The interest is installed only on in-session create/join

The kind:445 group-message interest is pushed by exactly one choke point:

- `crates/nmp-marmot/src/projection/state.rs:467` `subscribe_group_messages()` → `group_message_interests()` → `app.push_interest()` (state.rs:471-472)
- which is called only by `cache_group_relays()` (state.rs:440)

`cache_group_relays` has only three callers, all in-session ops in `crates/nmp-marmot/src/projection/ops.rs`:
- `create_group` (ops.rs:418)
- `accept_welcome` (ops.rs:549)
- gift-wrap/welcome ingest (ops.rs:633)

### Restart path installs no group interest

`register_with_keys` (`crates/nmp-marmot/src/ffi.rs:287`) pushes only the gift-wrap inbox interest at ffi.rs:480 (`giftwrap_inbox_interest`). There is **no** "for each joined group, push its message interest" loop — the structural asymmetry the hypothesis predicted. The autopublish tail (`crates/nmp-marmot/src/ffi/autopublish.rs`) only republishes the key package.

### The in-memory relay cache is empty on restart

`Inner.group_relays` (state.rs:114) is a plain in-memory `HashMap`, initialized empty in `MarmotProjection::new` (state.rs:192). It is never seeded from the store on construction. So even publish-side routing for those groups would degrade until a new in-session event refilled it.

### The relays ARE recoverable from the store (fix is feasible)

MDK persists group relays:
- `create_group` writes them via `replace_group_relays` (mdk-core groups.rs:1681)
- welcome accept writes them (groups.rs:2245)
- `MDK::get_relays(group_id) -> BTreeSet<RelayUrl>` reads them back (groups.rs:1451) from the SQLite `group_relays` table

So on restart we can enumerate `get_groups()`, call `get_relays()` per group, and re-drive the existing `subscribe_group_messages` / `cache_group_relays` choke point. No new persistence is needed.

### Crux answers
1. Per-group interest creation: state.rs:467-473 (the only kind:445 push site).
2. On startup: **NO** — only in-session created/joined groups get the interest; restart re-pushes nothing for stored groups. This is the bug.
3. `MarmotService::new`/`from_storage` (service.rs:242/266) does not enumerate existing groups or re-subscribe.
4. Giftwrap inbox IS re-pushed (ffi.rs:480); there is no group-message equivalent — the asymmetry.
5. Gap is kernel-side (Rust). iOS shell only renders `nmp.marmot.messages`; with no subscription the projection never updates (the unread badge bumps off the kind:1059 inbox/metadata path, but the 445 body never arrives). Not a shell bug.
6. Group messages are fetched from the group's relays (relay-pinned, `interest.rs:103` `relay_pin`), not user relays. That relay set is persisted by MDK but never restored+subscribed after restart.

This is exactly the live-leg analogue of the store-leg cache-serve gap fixed in #1237.

---

## PR-by-PR fix plan (small, independently mergeable; Rust owns all logic; D8 no polling)

**PR-1 — `MarmotService::group_relays` accessor (pure read seam).**
Add `pub fn group_relays(&self, group_id: &GroupId) -> Result<Vec<RelayUrl>>` to `crates/nmp-marmot/src/service.rs`, delegating to `self.mdk.get_relays(group_id)`. Keeps `nmp-marmot` the sole MDK importer; mirrors the existing `get_groups`/`get_members` read projections (service.rs:551-575). Unit test: file-backed `MdkSqliteStorage::new(path,...)` → create group with relays → drop service → reopen same path → assert `group_relays` returns the same set. This proves restart-recoverability and is the load-bearing fact the rest depends on.

**PR-2 — `MarmotProjection::resubscribe_all_groups` over the SAME choke point.**
Add a method on `MarmotProjection` (state.rs) that, under the lock, enumerates `service.get_groups()`, and for each calls `service.group_relays(&gid)` and routes the result through the existing `InnerHandle::cache_group_relays` (state.rs:431) — which both seeds the in-memory `group_relays` cache AND calls `subscribe_group_messages` (the uniform install path #1237-style). No new interest-push code: reuse `subscribe_group_messages`/`group_message_interests` verbatim so there is one install choke point, no per-call special-casing. Empty relay set → skip (matches existing `cache_group_relays` empty-guard at state.rs:432). Cheap, one-shot, non-blocking (D8).

**PR-3 — call it from the register tail.**
In `register_with_keys` (`crates/nmp-marmot/src/ffi.rs`), after `set_app` + interest/parser/observer registration and right after the giftwrap inbox push (ffi.rs:480), call `projection.resubscribe_all_groups()`. This is the symmetric "for each joined group, re-push its message interest" the startup path is missing. Idempotent: interest ids are deterministic (`group_message_interest_id`, interest.rs:43) and the kernel de-dupes, so re-register/account-switch is safe.

**PR-4 (optional hardening) — withdraw on unregister.**
For symmetry on sign-out/account-switch, withdraw the group-message interests in `nmp_marmot_unregister` (ffi.rs:567) so a switched-away account stops tailing foreign group relays. Lower priority; the de-dupe makes the receive-fix correct without it, but it's the clean lifecycle counterpart.

---

## Regression test (two-session)

Add to `crates/nmp-marmot/src/tests.rs` (or a new `restart_resubscribe_tests.rs`), file-backed so restart is real (in-memory storage gives a private handle per the note at tests.rs:180, so it cannot model restart — must use `MdkSqliteStorage::new(tempdir_path, ...)` reused across two `MarmotService`/`MarmotProjection` instances):

1. Session 1: build service over a temp file path; create a group with explicit relays; assert `get_relays` non-empty.
2. Drop session 1 (simulated app exit).
3. Session 2: build a fresh `MarmotService` + `MarmotProjection` over the SAME file path; call `resubscribe_all_groups`; assert the group-message interest(s) for each stored group were pushed (capture via a test `NmpApp`/interest-capture seam, or at minimum assert `resubscribe_all_groups` enumerates the persisted relays returned by `group_relays`, matching the set `subscribe_group_messages` would iterate).
4. Drive a new kind:445 from the peer through `MarmotIngestParser`/`ingest_signed_event_core` and assert the restarted projection's messages projection now contains it (the full live-receive proof). This mirrors the existing two-party round-trip in `crates/nmp-marmot/src/ffi/tests.rs`.

Always also run `cargo test -p nmp-testing --test doctrine_lint_smoke`. Scope cargo tests to `-p nmp-marmot` only; never workspace-wide.

## Device-harness verification
Reproduce the exact device scenario: A (iOS), B (Android), `NMP_TEST_NSEC`, `wss://relay.primal.net`. Join group, exchange a message (proves pre-restart receive), force-stop + relaunch both, send a new message from B. Pre-fix: A never decrypts it (current bug). Post-fix: A receives it within one relay round-trip after relaunch, with no chat re-open/tab-switch needed. The host TUI repro (`apps/chirp/chirp-tui` + `NMP_MARMOT_MOCK_KEYRING=1`) can stand in by constructing a second `MarmotService`/projection over the same SQLite file and feeding it a freshly-minted peer kind:445.

## Note
I did not modify any production code (debugging agent scope). No worktree changes were made; this is diagnosis + plan only. The fix builds directly on the merged marmot-create-fix ladder (#1227/#1237/#1230/#1235/#1239) and reuses the #1237 single-install choke point pattern for the live leg.

