# Codex review — #1493 P9 PR1 (operator relays + seed follows out of NMP)

Date: 2026-06-18. Reviewer: codex (gpt-5-codex). Branch: fix/1493-p9-relays-pubkeys.

## Design verdicts incorporated (pre-implementation)
- Q1 (known-signers source of truth) — deferred to PR2.
- Q2 (relays/pubkeys out of NMP) — DEFAULT_FOLLOWS → initial_follows param on
  ActorCommand::CreateAccount (empty → no kind:3); DEFAULT_APP_RELAYS deleted,
  builder type-state requires .with_relays()/.without_initial_relays();
  nostrconnect_bootstrap_relay → Option<String> default None; crate-boundaries.md
  §9 reclassifies nmp-defaults as a reusable library that owns no operator policy.
- Q3 (initial_follows seam) — generic C-ABI unchanged (dispatches empty follows);
  Chirp-owned nmp_app_chirp_create_new_account injects chirp_default_follows();
  no post-create action (cold-start kind:3 routing must stay in create_account).
- F6 (chirpConfig.ts role drift) — converge content relay to "both" (Rust is
  source of truth); web generation from Rust is a follow-up.

## Diff review findings + resolution
- HIGH chirp-tui runtime_commands.rs: still called generic create symbol →
  FIXED, now calls nmp_app_chirp_create_new_account.
- HIGH chirp-desktop bridge.rs: routed through the generic (handler-less)
  "nmp.create_account" action envelope → FIXED, now routes through the Chirp
  C-ABI wrapper (this also fixes a pre-existing latent no-op: no ActionModule
  is registered for "nmp.create_account").
- MEDIUM nmp-core relay.rs test fixtures (relay.primal.net/seed pubkeys under
  cfg(test)/test-support): LEFT AS-IS — aim.md §"Anti-patterns" explicitly
  permits relay/pubkey literals in test fixtures, TUI render, and CLI output.
- LOW nmp-defaults lib.rs stale rustdoc (still claimed damus.io default) → FIXED.

Clean on: type-state ptr::read+mem::forget safety, nostrconnect None fail-closed,
with_relays(empty) panic-as-misuse, doctrine (D0/D26), ffi-header-drift, file-size.
