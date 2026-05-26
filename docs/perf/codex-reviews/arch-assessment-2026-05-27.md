# Architectural Assessment - nostr-multi-platform (2026-05-27)

Audit base: `f0ca1643cb069a99ef625146eb90a28f6347b635`

## Verdict

**ARCHITECTURE IS NOT YET IN VERY GOOD STANDING.**

No P0 issue was found in this pass. Two P1 issues still block a "very good standing"
verdict:

1. `nmp-core` substrate still exposes a NIP-17-named host seam.
2. A kernel projection still imports `display::` formatting directly.

The executable gates passed: doctrine lint smoke, C-ABI surface freeze, and FFI header
drift.

## P1 Findings

### P1 - D0: `AppHost` substrate exposes a NIP-17-named observer seam

`crates/nmp-core/src/substrate/app_host.rs:101` defines
`swap_nip17_dm_inbox_observer` on the reusable `AppHost` substrate trait. That makes a
protocol noun part of the generic host-composition API, directly conflicting with the
crate-boundary rule that `nmp-core` is "Zero NIP knowledge" (`docs/architecture/crate-boundaries.md:316`)
and the D0 statement that the kernel/substrate must not name NIP nouns
(`docs/architecture/crate-boundaries.md:90`).

Supporting citations:

- `crates/nmp-core/src/substrate/app_host.rs:101` - NIP-17-named trait method on `AppHost`.
- `crates/nmp-ffi/src/lib.rs:1577` - concrete `NmpApp::swap_nip17_dm_inbox_observer` method.
- `crates/nmp-ffi/src/lib.rs:1949` - `AppHost` implementation forwards the NIP-17-named method.
- `crates/nmp-app-template/src/runtimes.rs:90` - app-template composition calls the NIP-17-named seam.
- `crates/nmp-core/src/slots.rs:158` - slot documentation still names "NIP-17 DM-inbox".

Proposed fix: rename the host seam and concrete method to a substrate noun such as
`swap_dm_inbox_observer` or a typed singleton-observer slot that does not encode the NIP
number in the API. Keep protocol-owned names inside `nmp-nip17` and app projection keys only.

### P1 - D6 audit rule: publish-outbox projection imports `display::`

The required display grep found production projection code:

- `crates/nmp-core/src/kernel/publish_outbox.rs:7` imports `crate::display::short_npub`.
- `crates/nmp-core/src/kernel/publish_outbox.rs:181` formats
  `RelaySelectionReason::RecipientInbox` as `"Inbox relay for <short npub>"`.

Per this assessment's D6 rule, `display::` hits in projection/snapshot/FFI code are
violations. This one is in a kernel projection that serializes user-visible relay reasons.

Proposed fix: carry structured relay-reason data across the projection boundary
(`reason_kind`, raw `pubkey`, relay URL, etc.) and let the shell or a dedicated display
layer format the short pubkey. If the intended doctrine is that `nmp_core::display` is
allowed in projections, reconcile the assessment rule with
`docs/architecture/crate-boundaries.md:1096`, which currently lists `display::` helpers as
shared substrate.

## P2 Findings

### P2 - D0 token hygiene: additional NIP-token hits remain in substrate/kernel text

The D0 grep produced many `substrate/` and `kernel/` hits. Most are comments or documented
NIP-42 wire-layer exceptions, not new dependency edges, but they are still D0-noisy.
Examples:

- `crates/nmp-core/src/kernel/auth.rs:36` and `crates/nmp-core/src/kernel/auth.rs:50`
  use `nmp_nip42_types` in production code. `docs/architecture/crate-boundaries.md:316`
  currently allows `nmp-nip42-types` as an `nmp-core` dependency, so I did not escalate this
  above P2.
- `crates/nmp-core/src/kernel/status.rs:340` and `crates/nmp-core/src/kernel/status.rs:345`
  emit user-facing `"nip65"` coverage labels.
- `crates/nmp-core/src/substrate/routing.rs:169` and
  `crates/nmp-core/src/substrate/routing.rs:304` are substrate docs that name NIP-17/NIP-29/NIP-65.
- `crates/nmp-core/src/kernel/ingest/mod.rs:441` names "per-NIP parsers" in a kernel ingest comment.

Proposed fix: keep code-level exceptions explicit and rename comments/status text to
substrate nouns where possible (`relay-list`, `mailbox`, `auth`, `dm-inbox`) so grep can
again be a useful first-pass D0 guard.

### P2 - V-57 P2 kind-constant drift remains outside `kinds.rs`

The magic-kind grep still finds production integer/string sites outside
`crates/nmp-core/src/kinds.rs`. This aligns with `docs/BACKLOG.md:57`, where V-57 P2 Stage 2
is still open, but one `nmp-core` production straggler remains too.

Representative production hits:

- `crates/nmp-core/src/kernel/requests/startup.rs:28` - `SELF_KINDS_TAILING` still uses
  literal `10002`.
- `crates/nmp-router/src/publish_relay_list.rs:75` - private duplicate
  `KIND_RELAY_LIST: u32 = 10002`.
- `crates/nmp-router/src/nip65_resolver.rs:150` - scans by literal `[10002]`.
- `crates/nmp-content/src/embed_projection/mod.rs:93` and
  `crates/nmp-content/src/embed_projection/mod.rs:112` - kind dispatch literals `9802`
  and `30023`.
- `crates/nmp-content/src/mode.rs:44` - markdown mode dispatch literals.
- `crates/nmp-planner/src/interest.rs:201` - profile discovery kinds include literal
  `10002`.
- `crates/nmp-marmot/src/interest.rs:24` and `crates/nmp-marmot/src/projection/ops.rs:633`
  still define/use literal `1059`.
- `crates/nmp-repl/src/discovery.rs:217` and `crates/nmp-repl/src/fanout.rs:157` still use
  literal `10002`.

Proposed fix: complete V-57 P2 Stage 2 by importing/re-exporting the canonical
`nmp_core::kinds::*` constants where dependency direction allows it. For crates where that
would create an invalid edge, add an explicit boundary note instead of retaining silent
duplicates.

### P2 - File-size ceiling still has broad known debt

The required top-20 command reported these largest Rust files:

```text
2243 crates/nmp-core/src/actor/commands/tests.rs
2149 crates/nmp-ffi/src/lib.rs
2021 crates/nmp-core/src/kernel/mod.rs
1928 crates/nmp-core/src/actor/dispatch.rs
1745 crates/nmp-core/src/actor/mod.rs
1495 crates/nmp-nostr-lmdb/src/store/lmdb/mod.rs
1360 crates/nmp-testing/bin/doctrine-lint/tests.rs
1224 crates/nmp-core/src/actor/commands/identity.rs
1194 crates/nmp-ffi/src/action/tests.rs
1178 crates/nmp-core/src/transport/generated/nmp_update_generated.rs
1087 crates/nmp-core/src/kernel/update.rs
964 crates/nmp-core/src/kernel/state_projection_tests.rs
936 crates/nmp-testing/bin/doctrine-lint/main.rs
933 crates/nmp-core/src/kernel/types.rs
917 crates/nmp-core/src/subs/lifecycle_tests.rs
887 crates/nmp-core/src/kernel/tests.rs
875 crates/nmp-nostr-lmdb/src/lib_tests.rs
839 crates/nmp-core/src/kernel/publish_terminal_status_tests.rs
801 crates/nmp-core/src/actor/commands/publish.rs
786 crates/nmp-core/src/kernel/ingest_tests.rs
```

A follow-up full `>500` scan found 56 files over the hard ceiling. Generated and fixture
files are exempt, but many production and hand-authored test files remain above 500 LOC.
This is known debt under `docs/BACKLOG.md:317`; the production list there is partly stale
because it still cites `crates/nmp-core/src/ffi/mod.rs` (`docs/BACKLOG.md:347`), while the
current offender is `crates/nmp-ffi/src/lib.rs`.

Proposed fix: keep V-12 active and refresh its production offender list before starting
new splits.

## Clean Checks

### D3 routing

The assessment's path `crates/nmp-core/src/ffi/mod.rs` no longer exists after FFI extraction.
Scanning `crates/nmp-ffi/src` for `kernel.` found comment-only hits, not direct
`kernel.<field>` reads or writes outside the dispatch path.

### D8 no-polling

The exact sleep grep printed six lines, but all are under `#[cfg(test)]` test modules:

- `crates/nmp-core/src/actor/tick.rs:91` opens the cfg-test module containing the sleep
  calls reported at lines 115, 157, 231, 293, and 306.
- `crates/nmp-ffi/src/lifecycle.rs:144` opens the cfg-test module containing the sleep
  call reported at line 317.

No production sleep loop was found by this pass.

### Executable gates

```text
cargo test -p nmp-testing --test doctrine_lint_smoke
test result: ok. 42 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 14.44s

bash ci/check-ffi-surface-freeze.sh HEAD HEAD
ffi-surface-freeze: OK - no new nmp_app_* per-verb exports.

bash ci/check-ffi-header-drift.sh
ffi-header-drift: OK - 61 production nmp_app_* symbols in sync.
```

## Backlog Audit

No open V-57 P1 remains: `docs/BACKLOG.md:50` says the P1 upward-dependency/app-policy
slice is closed, and `docs/BACKLOG.md:56` says the next V-57 item is P2.

Open P0/P1 backlog items that are not marked DONE and are not explicitly post-v1 deferred:

- P0: V-01 remains critical/staged (`docs/BACKLOG.md:107`) and F-01's IndexedDB store is
  still a V1 blocker (`docs/BACKLOG.md:1630`, `docs/BACKLOG.md:1642`).
- P0: F-02 DM cold-start receive-side verification remains a V1 blocker
  (`docs/BACKLOG.md:1652`, `docs/BACKLOG.md:1664`).
- P0: F-04 Zap E2E round-trip verification remains a V1 blocker
  (`docs/BACKLOG.md:1687`, `docs/BACKLOG.md:1699`).
- P1: V-37 snapshot output seam for non-Chirp apps remains HIGH and blocks PD-033-A
  (`docs/BACKLOG.md:780`, `docs/BACKLOG.md:819`).
- P1: V-42's NIP-51 mute-list slice is HIGH and v1-A relevant, while the other NIPs in
  that entry are post-v1 (`docs/BACKLOG.md:1025`, `docs/BACKLOG.md:1032`).
- P1: V-46 built-in projection cluster remains HIGH/pre-v1
  (`docs/BACKLOG.md:1113`, `docs/BACKLOG.md:1122`).
- P1: V-51 still has HIGH phase-3 UI work not started
  (`docs/BACKLOG.md:1235`, `docs/BACKLOG.md:1269`).
- P1: V-52 single-relay browsing remains HIGH/v1 DX
  (`docs/BACKLOG.md:1371`, `docs/BACKLOG.md:1392`).
- P1: F-CR-05 iOS kind registry remains HIGH
  (`docs/BACKLOG.md:1833`, `docs/BACKLOG.md:1849`).
- P1: F-CR-07 Android kind registry remains HIGH
  (`docs/BACKLOG.md:1867`, `docs/BACKLOG.md:1879`).

Known post-v1 HIGH items excluded from the above list: V-38 and V-50.

## Command Coverage

Commands run from the audit worktree:

```text
grep -r "nip29\|nip17\|nip42\|nip57\|nip47\|nip65" crates/nmp-core/src/ --include="*.rs" -l
grep -rn "kernel\." crates/nmp-ffi/src --include="*.rs"
grep -r "display::" crates/nmp-core/src/ --include="*.rs" -l
grep -r "display::" crates/nmp-ffi/src/ --include="*.rs" -l
grep -rn "thread::sleep\|std::thread::sleep" crates/ --include="*.rs" | grep -v "test\|#\[cfg(test\|//.*sleep"
find crates/ -name "*.rs" | xargs wc -l 2>/dev/null | sort -rn | head -20
grep -rn "\b1059\b\|30023\|9802\b\|10002\b" crates/ --include="*.rs" | grep -v "kinds\.rs\|#\[cfg(test\|test\|fixture\|//\|mod\.rs.*macro\|\.toml"
cargo test -p nmp-testing --test doctrine_lint_smoke 2>&1 | tail -20
bash ci/check-ffi-surface-freeze.sh HEAD HEAD 2>&1 | tail -5
bash ci/check-ffi-header-drift.sh 2>&1 | tail -5
```
