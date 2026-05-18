# 21 — The framework-magic contract

> Status: **SHIPS**. Audience: builders. The contract is the explicit list of
> operations the framework does so your app never has to. The doc is
> `docs/design/framework-magic.md:24-72`; the proof is
> `crates/nmp-testing/tests/framework_magic_contract.rs`; this section is the
> builder-facing distillation.

The framing (`docs/design/framework-magic/intro.md` §1) is the user's:
*"apps shouldn't have to care or know about these operations happening in the
background, things should just work."* The contract enumerates 13 such
behaviors (C1–C13). Each binds a behavior → the doctrine it discharges → the
owning milestone → a named test. There is no app-side surface on which to
express the broken version of any of them.

## C1–C13 — what the app gets for free

The **status** column below is derived from *both* the index table in
`docs/design/framework-magic.md:30-42` *and* the actual test file at master
tip. The test file
(`crates/nmp-testing/tests/framework_magic_contract.rs:1-25`) states *"All 14
tests are active. M2/M4/M6/M8 milestones are DONE on master"* and contains
**zero `#[ignore]`** attributes across its five sub-modules
(`c1_c4_c6_c9.rs`, `c5_c8_c13.rs`, `c7_c11.rs`, `c10.rs`, `c12.rs`). The
owning milestones M0–M8 are DONE per the orchestration log. The status here is
**evidence at master tip**, not the stale `[PENDING M_n]` text still carried in
the design doc's index table — that mismatch is recorded in
[27 — Doc/code discrepancies](27-discrepancies.md).

| # | Behavior | What the app gets for free | Doctrine | Test | Status |
|--:|---|---|---|---|---|
| C1 | Replaceable supersession (kind 0/3/10000–19999) on insert | Newest kind:0/3 wins automatically; no app-side "is this newer?" check | D1 | `c1_replaceable_supersedes_on_insert` | **[DONE]** (active) |
| C2 | Parameterized replaceable supersession (30000–39999) by `(pubkey, kind, d-tag)` | Per-`d`-tag slots stay independent; no app-side keying logic | D1 | `c2_parameterized_replaceable_supersedes_by_dtag` | **[DONE]** (active; doc table says `[PENDING M3]` — drift §27) |
| C3 | Kind:5 delete: referenced events removed, tombstone persisted | Deletes propagate; cross-author deletes ignored; no app delete-by-id | spec §7.1 | `c3_kind5_delete_removes_referenced_and_tombstones` | **[DONE]** (active; doc says `[PENDING M3]` — drift §27) |
| C4 | NIP-40 expiration auto-removes at expiry; survives actor restart | Expired events vanish on schedule, even across restart; no app timer | spec §7.1 | `c4_nip40_expiration_removes_and_persists_schedule` | **[DONE]** (active; doc says `[PENDING M3]` — drift §27) |
| C5 | Kind:3 auto-tracking: follow-list change recompiles dependent subs | Follow someone → their notes appear; no Svelte rune / React dep wiring | D3 | `c5_kind3_change_recompiles_follow_dependent_subs` | **[DONE]** (active; doc says `[PENDING M2]` — drift §27) |
| C6 | Outbox read routing: `authors` filters fan out to write relays, deduped | Reads reach the right relays; no relay-set bookkeeping in the app | D3 | `c6_authors_subscription_routes_to_per_author_write_relays` | **[DONE]** (active; doc says `[PENDING M2]` — drift §27) |
| C7 | Outbox write routing + private events fail closed on unknown inbox | Publishes reach author write + `#p` inbox; gift-wrap fails safe | D3 | `c7_publish_routes_outbox_and_private_fails_closed` | **[DONE]** (active; doc says `[PENDING M2→M6]` — drift §27) |
| C8 | Planner dedups overlapping interests, auto-closes on EOSE/last-drop, buffers ≤60Hz/view | One wire REQ per relay; views close themselves; no manual dedup | spec §7.2 | `c8_subscriptions_coalesce_autoclose_and_buffer` | **[DONE]** (active; doc says `[PENDING M2]` — drift §27) |
| C9 | Provenance merge: same id from N relays → one event, N-entry provenance | Dedup across relays; original `id`/sig untouched; no app merge | aim §6 d.10 | `c9_provenance_merges_across_relay_redeliveries` | **[DONE]** (active; doc says `[PENDING M3]` — drift §27) |
| C10 | Sync watermarks gate backfill; full coverage → authoritative miss; NIP-77 default | Cache-miss is trustworthy when covered; no manual REQ scans | D2 | `c10_watermark_gates_backfill_and_authoritative_miss` | **[DONE]** (active; doc says `[LANDED M4]`) |
| C11 | Signer onboarding: `bunker://` parse+connect, "create nsec" gen+NIP-49+persist | Paste a bunker URL or make a key — both as kernel actions, no app code | spec §7.4 | `c11_bunker_url_and_nsec_creation_complete_via_actions` | **[DONE]** (active; doc says `[PENDING M6]` — drift §27) |
| C12 | Account switch is a state transition: views re-resolve, no CLOSE/REQ dance | Switch account → every scoped view rebinds itself | D4 | `c12_account_switch_rebinds_views_without_imperative_dance` | **[DONE]** (active; doc says `[PENDING M8]` — drift §27) |
| C13 | Best-effort rendering: every payload field non-`Option`, placeholders, in-place refine | Render now, refine in place; no `if has_profile { spinner }` surface | D1 | `c13_view_payload_uses_placeholders_then_refines_in_place` | **[PARTIAL]** placeholder shape DONE; in-place enrich gated (see note) |

Plus the 14th test, `contract_surface_complete` — a **coverage meta-test**
(not `#[ignore]`, runs every CI) that parses the table in
`framework-magic.md:30-42` and asserts every row has a matching `#[test] fn`
name in `EXPECTED_TESTS`
(`crates/nmp-testing/tests/framework_magic_contract.rs:38-98`). It catches
*structural* drift (a row without a test, a test without a row, a rename) — it
deliberately does **not** assert `[DONE]`/`#[PENDING]` text accuracy, which is
why the status-text drift in the table above slipped past it and is filed in
[27](27-discrepancies.md).

### The C13 nuance

C13 is the one bullet whose own design chapter
(`docs/design/framework-magic/capabilities.md:45-47`) still narrates a
`[PARTIAL]` / `[PENDING M2/M3]` story: placeholder *shape*
(`author_display` as non-`Option` shortened-npub) is DONE and asserted by
sub-paths 1 + 5; the `author_picture_url: Option<String>` → non-optional
identicon fix and the on-enrich in-place refinement (sub-paths 2/3/4) are
described there as gated. The behavior **test** `c13_…` is active and not
ignored at master tip (`c5_c8_c13.rs:237-238`), so the contract bullet itself
ships; the *prose chapter* is the lagging artifact. Treat C13 as **[PARTIAL]**
until that chapter's "framework-magic delta" lands — also noted in
[27](27-discrepancies.md).

## Why you do not write fallback code

The contract's value to a builder is negative: it is the list of code you must
**not** write. There is no `if event_is_newer { replace }` (C1), no
`recompileSubsOnFollowChange()` (C5), no `dedupeAcrossRelays()` (C9), no
`if (!profile) showSpinner()` (C13). The API does not expose the question that
would justify the fallback. Writing it anyway is the anti-pattern; it will
diverge from the kernel's authoritative behavior and reintroduce exactly the
bug class the bullet extinguishes.

## Mini-recipe — adding C14

The contract is *append-stable*: adding a bullet is allowed, removing one needs
an ADR (`docs/design/framework-magic/intro.md` §4). To add C14:

1. **Write the ADR.** `docs/decisions/00NN-c14-<slug>.md`, sections:
   *Context* (which doctrine clause / bug class it discharges) ·
   *The guarantee* (one sentence: what the app no longer writes) ·
   *App writes* (must be `nothing` or one safe call — if not, D0 is violated,
   stop) · *Mechanism* (which existing crate owns it; no new `nmp-core`
   nouns) · *Test name* (see step 3) · *Owning milestone*.
2. **Add the index row** to `docs/design/framework-magic.md` (the `| C14 |
   … |` row) and a chapter sub-file under `docs/design/framework-magic/` using
   the six-field template in `intro.md` §3 (Framework does / App writes /
   Failure mode prevented / Test / Milestone owner).
3. **Test-name convention** (`docs/design/framework-magic/test-scaffolding.md`
   §2): `c14_<snake_case_behavior>` — lowercase `c`, the number, then a
   verb-led snake_case phrase that names the *observable*, not the milestone
   (e.g. `c14_blossom_upload_routes_and_dedups`). Names are **stable
   identifiers**; a rename is a contract revision requiring a deprecation
   shim (`#[test] fn old() { c14_new() }` for one milestone cycle).
4. **Add the `#[test] fn`** in a `framework_magic_contract/` sub-module and the
   name to `EXPECTED_TESTS` in `framework_magic_contract.rs`. The
   `contract_surface_complete` meta-test will fail the build until the row and
   the test name agree — that failure is the gate, by design.
5. **Record the framework-magic delta** in the owning milestone's exit-gate
   report (which bullets moved to `[DONE]`, which `#[ignore]` came off). A
   milestone landing that touches the contract without a delta is a structural
   defect per `intro.md` §4.

## Anti-patterns

- **Assuming the status column without checking both sources.** The design
  doc's index table is stale (`[PENDING M_n]`) while the test file is fully
  active. Always cross-check `framework_magic_contract.rs` (and its sub-modules
  for `#[ignore]`) against `framework-magic.md` before quoting a status; the
  mismatch itself is reportable §27 material.
- **Paraphrasing a bullet into a stronger claim.** C13 guarantees
  *placeholders + in-place refinement*, not "all profile data is instantly
  present". C10 guarantees *authoritative miss when covered*, not "everything
  ever is in cache". Overstating the contract re-creates the bug it prevents.
- **App-side fallback "just in case".** A `dedupeAcrossRelays()` or
  `replaceIfNewer()` helper in app code. C1/C9 already do this in the kernel;
  the app copy will drift and double-handle.
- **Re-implementing kind:3 watch in SwiftUI.** A `@StateObject` that subscribes
  to the contact list and re-opens views on change. C5 does this in the kernel;
  the SwiftUI copy fights the planner's recompile.
- **Renaming a contract test to fit the milestone-prefix convention.**
  `framework_magic_contract.rs` is intentionally *not* milestone-prefixed
  (`test-scaffolding.md` §1); renaming it or its tests breaks the meta-test and
  needs an ADR.

See also: [03 — Doctrine D0–D8 end-to-end](03-doctrine-d0-d8.md) ·
[08 — EventStore + insert invariants + GC](08-eventstore.md) ·
[10 — Outbox routing (NIP-65)](10-outbox-routing.md) ·
[11 — Sessions + signers + identity scopes](11-sessions-signers.md) ·
[18 — Testing — `nmp-testing`, benches, contract tests](18-testing.md)
