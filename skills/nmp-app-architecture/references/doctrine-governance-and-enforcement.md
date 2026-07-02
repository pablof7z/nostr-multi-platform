# Doctrine Governance and Enforcement

> Authority: ADR-0073 (ADR reset + rolling ratchets), `docs/decisions/README.md` (the ledger),
> `docs/escape-hatches.md`, and the doctrine-lint binary under `crates/nmp-testing/`.

## The Two-Tier Waiver System

Doctrine is enforced at two levels that do not substitute for each other:

1. **Per-line opt-out** — `// doctrine-allow: Dn — reason`. Silences doctrine-lint for one
   code line; the rule still applies everywhere else. Rule ID required; reason prose mandatory
   for D23/D24/D25 (bare `doctrine-allow: D23` is rejected). Multiple rules:
   `doctrine-allow: D6,D8 — reason`. Must be a trailing comment on the same line as the
   offending code. Because rustfmt moves trailing comments off fn declarations, use
   `#[rustfmt::skip]` on the fn item when the allow must sit on a fn signature.

2. **ADR** — the only mechanism for an *architectural* exception (a structural deviation across
   a module, crate, or design boundary). "No ADR means no waiver." Silent exceptions are
   violations.

## What Doctrine-Lint Enforces (Rust + native)

`cargo run -p nmp-testing --bin doctrine-lint -- --crate nmp-core` plus the smoke test. The
binary enforces **A6, D0, D6–D15, D17–D27, action_namespace, no_raw_tap, product_raw_read**
(D16 was deleted when Chirp was extracted to its own repo — its scope was `apps/chirp/`
exclusively and that path no longer exists in this monorepo; D22 is a reserved/unassigned
slot). The "D0–D10" framing is obsolete; the live set runs to D27.

| Rule | Summary | Scope |
|---|---|---|
| D11 | No bespoke `nmp_app_publish_*` FFI doors | all |
| D12 | Async-completing ActionModules record stages | all |
| D13 | DM-path raw-key isolation | nmp-core |
| D14 | Typed snapshot projection slots (no bare `Mutex<Vec>`) | nmp-core |
| D15 | Host closures wrapped in `catch_unwind` | nmp-core |
| D17 | Social-timeline kinds not hardcoded in substrate | nmp-core |
| D18 | Native shell doctrine (Swift/Kotlin/Java) | all native |
| D19 | Display formatting banned from kernel projection producers | nmp-core |
| D20 | No raw `Instant`/`SystemTime` on the wasm path | wasm |
| D21 | No ambient authority (K2 regression gate) | scoped |
| D23 | Single accepted-event store-insert chokepoint | event flow |
| D24 | Single post-store observer fan-out seam | event flow |
| D25 | Single REQ-build door (acquisition one-door) | scoped |
| D26 | No ambient authority in protocol/command code | protocol |
| D27 | Banned display helpers in projection/snapshot/FFI | scoped |
| product_raw_read | No `open_interest`/`ObservedProjection`/raw observers in apps | apps/*, templates |
| nip29_kind_blind | `nmp-nip29` owns only the h-tag envelope — no foreign-kind literals or kind-named actions | nmp-nip29 |

CI runs: binary against `nmp-core`; binary against `nmp-browser-runtime`; `--workspace-d8`
(no-polling D8 across all production crates + apps); `--workspace-native` (D18 across
Swift/Kotlin/Java); the smoke test; and native smoke + rule unit tests.

**Always run locally before a PR:**
```bash
cargo test -p nmp-testing --test doctrine_lint_smoke
cargo run -p nmp-testing --bin doctrine-lint -- --crate nmp-core
```

## Doctrine-Lint vs the Architecture Scanner

The Python scanner (`scripts/nmp_architecture_scan.py`) is triage; doctrine-lint is the gate.
Their division of labor:

- **Doctrine-lint owns the Rust D-rules.** Do not duplicate D0, D6, D8 no-polling, or D11–D27
  in the scanner.
- **The scanner's comparative advantage is cross-language heuristics** (Swift/Kotlin/TS/Java)
  that doctrine-lint does not scan, plus checks doctrine-lint has no rule for yet (D1 loading
  gates, `register_defaults` call sites, layer-inversion display-in-wire) and checks that must
  run on *external* app repos that do not build the doctrine-lint binary.

When you add a scanner rule, ask: does doctrine-lint already enforce this in Rust? If yes,
leave it to doctrine-lint. If it is a cross-language or external-repo concern, the scanner is
the right home.

## ADR Directory Governance

`docs/decisions/` contains only decisions that still govern the current
architecture. It is not an archive. When a rule stops being current, move any
surviving invariant into its current owner and delete the obsolete ADR file.
Git history, closed issues, and pull request bodies preserve earlier context.

The active redesign spine is **ADR-0069 through ADR-0073**. Extensions such as
ADR-0074 through ADR-0076 remain only while they own live invariants that do not
belong cleanly in the spine or a durable architecture/API document.

When a PR touches an architectural invariant, update the current owner **in
place**. Do not add a parallel correction document that leaves stale guidance
behind, and do not preserve obsolete ADR text for context.

## Rolling Ratchets

Each architecture slice that reduces old public surfaces carries a deletion
summary in its PR:

```
old public doors deleted or privatized:
old compatibility paths scoped:
new public concepts added:
net permanent concepts:
```

A slice is valid only if it reduces one or more of: permanent concepts, public doors, lifecycle
recipes, shell policy sites, duplicate owners — while preserving behavior. Progress is measured
by shrinking old surfaces and passing ratchets, not by landing new terminology. Standing
ratchet targets: `register_defaults()` in production, public `open_interest`/`ReducedSource`/
`ObservedProjection` in app shells, anonymous explicit relay routes, hidden projection tiers.

## Escape-Hatch Contract

Source: `docs/escape-hatches.md`. A capability the sound design cannot express through a typed
seam is a design gap to close, not a whitelist candidate. Current seams:

1. **ActionModule** (`NmpApp::register_action::<M>()`) — the preferred extension path, not a
   negative escape hatch. Use before any other seam.
2. **Test-only injectors** (`nmp_app_inject_*`) — mechanically gated by
   `#[cfg(any(test, feature = "test-support"))]`. Never in production ABI.
3. **IngestParser** (`register_ingest_parser` / `replace_ingest_parser`) — in-process state
   derivation from inbound signed events. Bypasses D3 dispatch; does not bypass D1 or D8.

Review gate: escape hatches must be **named, gated, instrumented, and tested**.
