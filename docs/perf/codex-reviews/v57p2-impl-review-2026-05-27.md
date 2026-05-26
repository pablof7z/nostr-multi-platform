# V-57 P2 — codex review (2026-05-27)

PR: https://github.com/pablof7z/nostr-multi-platform/pull/641
Branch: `worktree-agent-af4d489b9659839d0`
Reviewed commit: `bfcf5354` (initial slice, before follow-up `76eba86d`)

## Prompt

The prompt that produced this review lives at `/tmp/v57p2-impl-review.md`
during the session; condensed here:

> Review the V-57 P2 PR centralising Nostr kind constants in
> `nmp_core::kinds`. Check that:
> 1. all local `const KIND_GIFT_WRAP` / `KIND_RELAY_LIST` duplicates in
>    `nmp-core` were eliminated;
> 2. the new `kinds.rs` module's docstrings match the NIP each constant
>    claims;
> 3. there are no obvious logic errors;
> 4. the `publish.rs` doc-prose and `tracing::warn!` log no longer name
>    NIP-17, kind:10050, or Marmot (the `kind:1059` integer reference
>    is intentionally kept);
> 5. the protocol-crate private-duplicate migration is correctly
>    deferred to a follow-up;
> 6. the BACKLOG.md V-57 P2 entry update is accurate.

## Codex verdict

**Not LGTM** — three actionable findings against `bfcf5354`.

### 1. Production `10002` / `1059` literals still present in `nmp-core`

The new registry was not yet the canonical source of truth; codex listed:

- `crates/nmp-core/src/actor/commands/identity.rs:859`
- `crates/nmp-core/src/kernel/discovery.rs:180`
- `crates/nmp-core/src/subs/recompile.rs:147`
- `crates/nmp-core/src/kernel/requests/profile.rs:519`
- `crates/nmp-core/src/kernel/publish_outbox.rs:392`

### 2. `Marmot` reference in `publish_signed_event` doc-block

`crates/nmp-core/src/actor/commands/publish.rs:292` still mentioned
"NmpApp::publish_signed_explicit Marmot seam" — the substrate-neutral
rewording should have caught it.

### 3. BACKLOG.md V-57 P2 entry internally inconsistent

The prose said "centralised registry created in this slice" while the
"Next step" line said "create `nmp_core::kinds`" — the two halves
disagreed on what stage 1 had delivered.

## Codex confirmations (other checks)

- Protocol-crate duplicate migration correctly deferred — only
  `nmp-core` + `docs/BACKLOG.md` touched.
- `kinds.rs` constants match the NIPs they claim.
- `cargo check -p nmp-core` clean (with the pre-existing warnings).
- `cargo test -p nmp-core --lib` — 901 passed, 1 ignored.
- `cargo test -p nmp-testing --test doctrine_lint_smoke` — 42/42 pass.

## Follow-up applied (`76eba86d`)

All three findings addressed in commit `76eba86d`:

1. The five production `10002` / `1059` call sites migrated to
   `crate::kinds::KIND_RELAY_LIST` / `KIND_GIFT_WRAP`. Legacy NIP-04
   kind:4 and kind:44 in `publish_outbox.rs::publish_event_preview`
   left as file-local consts with a comment explaining
   `nmp_core::kinds` only mints constants for kinds the workspace
   actively WRITES.
2. The `Marmot` token in `publish.rs:292` replaced with
   "workspace-internal seam".
3. BACKLOG.md V-57 P2 entry rewritten — stage 1 is now explicitly
   marked DONE; stage 2 (protocol-crate private-duplicate migration)
   is the open next step.

Gates re-confirmed post-fix: `cargo check -p nmp-core`,
`cargo test -p nmp-core`, and `cargo test -p nmp-testing --test
doctrine_lint_smoke` all pass.
