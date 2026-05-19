# Parallel-Work Brainstorm — 2026-05-18

**Task:** T61 parallel-work-brainstorm — ranked list of high-value fan-out candidates for the orchestrator.

**State observed at authoring (orientation cap @ commit `7355474`):**
- HBs 0–5 in `docs/perf/orchestration-log.md`. The brief refers to "HB27 onwards" and milestones M2–M8 substrate landed; the working tree shows M0 ✅ / M1 🟡 / M2+M3 design landed + codex-fixed / M4–M8 still "scoped, pending design" per `docs/plan/status.md`. The brainstorm is written *forward* from the brief's stated state and is also robust to the actual state — items are filed against the eventual fan-out window. Where the brief claims M5–M8 substrate is in, I treat that as the assumption and provide items that build *on top of* that substrate; where it's not in, the same items also serve as the implementation candidates.
- In-flight (per brief): T56 nip29-integration-tests, T57 framework-magic-activate, T58 M5/M2/M8 wiring, T59 docs-planner, T60 e2e-ios-planner. All items below are explicitly marked **parallel-safe** or **coordinate-with-T##**.
- `pending-user-decisions.md` currently shows only PD-001 (resolved). Brief lists PD-002/003/004 as open — filed as a spec-gap item below.
- No `TaskCreate`/`TaskList` tool in this agent's environment; T61 status is recorded in the commit body for the orchestrator to ingest.

**Output budget:** ≤800 LOC, ~60 items targeted. Tier-1 uses the full 9-bullet spec; Tier-2 and Tier-3 use compact row format.

---

## TIER 1 — dispatch immediately (15)

Highest value × already-unblocked × parallel-safe with all in-flight work.

### 1. Real-relay capability fingerprint sweep

- **Category**: networking / soak / observability
- **Scope**: Build a `nmp-testing/bin/relay-fingerprint` binary that queries 50+ known public relays for NIP-11 `relay info`, supported NIPs, auth requirement, write-only/read-only mode, payment status, max-message-size, max-subscriptions, max-filters. Emits a JSON ledger + a markdown matrix.
- **Why now**: every milestone after M4 needs to know which relays advertise NIP-77, NIP-42, NIP-65, NIP-29. Fingerprint corpus is the substrate for all real-world test selection. Read-only; cannot regress anything.
- **Effort**: small
- **Model**: sonnet
- **Parallel-safe**: yes
- **Depends on**: none
- **Deliverable**: `crates/nmp-testing/bin/relay-fingerprint/` + `docs/perf/relay-fingerprint-2026-05-18.json|.md`. Commit stub: `testing(relay-fingerprint): NIP-11 capability sweep of 50 public relays`.
- **Risk if skipped**: real-relay testing chooses relays by guess; M4/M5/M6 hardening flies blind on which relays implement what.

### 2. Real-event corpus capture from public relays

- **Category**: testing / soak / correctness
- **Scope**: Add a `firehose-bench capture --duration=900 --relays=…` invocation set that pulls ~10 distinct corpora (high-volume kind:1, kind:7-heavy, kind:30023 long-form, kind:9 NIP-29 groups, profile-heavy kind:0, contact-list-heavy kind:3, repost-heavy kind:6, zaps kind:9735, deletion-rich kind:5, replaceable churn kind:10002). Store as `.jsonl` under `docs/perf/corpora/` with size + provenance metadata.
- **Why now**: every replay test (M2 plan recompiler, M3 EventStore, M4 negentropy, future fuzzers) needs realistic input. MockRelay's synthetic streams hide tag-shape pathologies.
- **Effort**: small-medium
- **Model**: sonnet
- **Parallel-safe**: yes
- **Depends on**: #1 (nice-to-have, not blocking)
- **Deliverable**: 10 corpora + `docs/perf/corpora/README.md` (provenance, license note, anonymization stance). Commit stub: `testing(corpora): capture 10 real-event corpora from public relays`.
- **Risk if skipped**: fuzz/replay tests never see naturally-occurring weirdness; bugs only surface in production.

### 3. NIP-19 entity reference parser + formatter (npub/nsec/nevent/naddr/nprofile/note)

- **Category**: NIP / correctness
- **Scope**: Implement bech32-tlv encode/decode for the six NIP-19 entity types as a self-contained module `nmp-core::nip19` (or feature-flagged in a `nmp-nip19` sub-crate). Round-trip property tests via `proptest`. Reject malformed inputs explicitly.
- **Why now**: every UI surface (profile rendering, share sheet, paste handling, deep links) needs this. Currently absent. Pure module — no kernel touch.
- **Effort**: small (3-4h)
- **Model**: sonnet
- **Parallel-safe**: yes
- **Depends on**: none
- **Deliverable**: module + ≥30 unit tests + ≥4 proptest invariants + rust-doc with worked examples. Commit stub: `core(nip19): bech32-tlv entity parser + formatter with proptest round-trip`.
- **Risk if skipped**: ad-hoc per-app implementations; iOS share-extension blocked.

### 4. NIP-21 `nostr:` URI scheme handler

- **Category**: NIP / DX
- **Scope**: Build `nmp-core::nip21::parse(uri) -> NostrUriTarget` enum (Profile / Event / Address / RelayHint) routing through the NIP-19 parser. Defines the canonical `KernelAction::OpenUri(NostrUri)` dispatch path.
- **Why now**: iOS Universal Links + Android intent-filters + desktop URL-handler all need a single canonical parser. Lays the groundwork for share extension, push notification deep links, and `nostr:` link clicks inside markdown.
- **Effort**: small
- **Model**: sonnet
- **Parallel-safe**: yes
- **Depends on**: #3 NIP-19 (will need to coordinate landing order; #3 first)
- **Deliverable**: module + tests + ADR draft for kernel-side URI dispatch. Commit stub: `core(nip21): nostr URI scheme parser + KernelAction::OpenUri`.
- **Risk if skipped**: every shell rolls its own; inconsistency leaks user-facing semantics.

### 5. File-size pre-commit hook + CI gate

- **Category**: CI / tooling
- **Scope**: Add `.githooks/pre-commit` (shell) that warns at 300 LOC and fails at 500 LOC on tracked non-generated Rust/Swift/MD/TS files. Mirror in `.github/workflows/file-size-gate.yml`. Allow-list for generated / bench artifacts via `.file-size-ignore`.
- **Why now**: AGENTS.md mandates 300 soft / 500 hard. Today the rule is unenforced — `kernel/requests.rs` is at 574 LOC (over the hard ceiling). A gate catches drift the moment it lands.
- **Effort**: small (2h)
- **Model**: sonnet
- **Parallel-safe**: yes (touches only `.githooks/`, `.github/workflows/`, root `.file-size-ignore`)
- **Depends on**: none
- **Deliverable**: hook + CI workflow + opt-in install line in `README.md` + first-pass `.file-size-ignore`. Commit stub: `tooling(file-size): pre-commit hook + CI gate enforcing AGENTS.md`.
- **Risk if skipped**: file bloat accelerates; the M11 podcast rebuild explodes file sizes silently.

### 6. Split `kernel/requests.rs` (574 LOC, breaches 500-LOC hard ceiling)

- **Category**: refactor
- **Scope**: Decompose `crates/nmp-core/src/kernel/requests.rs` into cohesive submodules (subscription requests / profile claim-release / interest registration / event-id requests). Keep the public re-export surface stable.
- **Why now**: explicit AGENTS.md violation today (`574 > 500`). Easier to split before T57/T58 add more.
- **Effort**: small-medium
- **Model**: sonnet
- **Parallel-safe**: **coordinate-with-T57 and T58** (both may touch this file). Split first, then T57/T58 patches apply cleanly to the smaller files.
- **Depends on**: pause T57/T58 for 30 min OR rebase them onto the split.
- **Deliverable**: 3-4 files ≤200 LOC each + `mod.rs` re-exports unchanged + tests still green. Commit stub: `refactor(kernel/requests): split 574-LOC file into cohesive submodules`.
- **Risk if skipped**: AGENTS.md violation persists; merge conflicts compound as T57/T58 grow it further.

### 7. `cargo audit` + `cargo deny` CI workflow

- **Category**: security / CI
- **Scope**: Add `.github/workflows/supply-chain.yml` running `cargo audit` (RUSTSEC advisories) and `cargo deny check` (license + duplicate-dep + advisory). Commit a `deny.toml` with sensible defaults.
- **Why now**: zero supply-chain visibility today. Free, fast, no runtime cost. Catches a vulnerable dep the day it's added.
- **Effort**: small (1-2h)
- **Model**: sonnet
- **Parallel-safe**: yes
- **Depends on**: none
- **Deliverable**: workflow + `deny.toml` + first-run report committed to `docs/perf/supply-chain-baseline.md`. Commit stub: `ci(supply-chain): cargo-audit + cargo-deny workflow + baseline`.
- **Risk if skipped**: silent vulnerable transitive dep; license-incompatibility risk.

### 8. Doctrine-lint static check (D6 / D7 / D8 grep gates)

- **Category**: compliance / tooling
- **Scope**: `crates/nmp-testing/bin/doctrine-lint` — grep-based static analyzer that fails on (a) `panic!` / `unwrap()` outside `#[cfg(test)]` in `nmp-core/src/` (D6); (b) policy-decision verbs in `substrate/capability.rs` ("retry", "fallback", "select"); (c) `Vec<Event>` allocation in hot-path modules (`ingest.rs`, `reactivity-bench`) (D8). Output: actionable diff suggestions.
- **Why now**: doctrines are aspirational without enforcement. A grep linter is 100 LOC, runs in 200ms, prevents drift forever.
- **Effort**: small (3-4h)
- **Model**: opus (design-heavy: choosing what to lint without false-positive storms)
- **Parallel-safe**: yes
- **Depends on**: none
- **Deliverable**: binary + per-rule docstring + `cargo run -p nmp-testing --bin doctrine-lint` CI step + 1 page in `docs/decisions/` documenting which rules fire. Commit stub: `compliance(doctrine-lint): D6/D7/D8 grep gates`.
- **Risk if skipped**: a future PR violates D6/D7/D8 invisibly; doctrines become aspirational.

### 9. NDK → NMP migration guide

- **Category**: docs / migration
- **Scope**: `docs/migration/ndk-to-nmp.md`. Take the 10 most-common NDK patterns from `docs/research/ndk/` (NDK init, sub creation, filter use, signer attach, outbox publish, kind:3 follow watch, profile fetch, event cache, relay set, NIP-04 DM), show side-by-side the equivalent NMP idiom.
- **Why now**: external adoption is gated on this doc. Cheap to write — research is done. iOS app M11/M11.5 builders are first migrants.
- **Effort**: small-medium
- **Model**: opus (high-leverage prose)
- **Parallel-safe**: **coordinate-with-T59** (builder-guide TOC owner). Land *inside* T59's TOC slot if T59 has produced the index, else stand-alone.
- **Depends on**: T59 outline (soft)
- **Deliverable**: single markdown file ≤300 LOC, every example compileable against current `nmp-core`. Commit stub: `docs(migration): NDK to NMP — 10-pattern side-by-side guide`.
- **Risk if skipped**: adoption blocker; every prospective user has to reverse-engineer.

### 10. MockRelay NIP-65 outbox-semantics mode

- **Category**: testing
- **Scope**: Extend `crates/nmp-testing` MockRelay with a `OutboxMode { author_write_relays: HashMap<Pubkey, Vec<RelayUrl>>, recipient_inbox_relays: ... }` so that subscription compilation tests can verify the planner correctly fans out to per-author write relays and recipient inbox relays. Two named scenarios: "follow-feed routes to follows' write relays" and "DM-style publish routes to recipient's inbox relays".
- **Why now**: M2 design has landed but is untested against a relay set that actually exposes per-author write relays. Property tests for outbox planning need this mock.
- **Effort**: small-medium
- **Model**: sonnet
- **Parallel-safe**: **coordinate-with-T58** (M2 wiring touches the same surface). Land MockRelay extension first; T58 then writes the assertions against it.
- **Depends on**: none (MockRelay itself ships)
- **Deliverable**: extension to `nmp-testing/src/lib.rs` (or new `outbox_mode.rs`) + 4 named test scenarios in `crates/nmp-testing/tests/`. Commit stub: `testing(mock-relay): NIP-65 outbox-semantics mode + 4 routing scenarios`.
- **Risk if skipped**: M2 outbox planner ships untested against the relay shape it's designed for.

### 11. Structured logging contract + tracing-spans audit

- **Category**: observability
- **Scope**: Define the structured-logging contract in `docs/decisions/0011-observability-contract.md`: every kernel actor message gets a `tracing::span!`, every FFI call gets a span, every relay frame gets a span. Audit `nmp-core/src/` for unspanned hot paths and file a fix-it list (don't fix in this task — fix-it issues spawn future work).
- **Why now**: today observability is `println!` and ad-hoc `log::debug!`. iOS empirical debugging is unproductive without structured spans. M10.5 needs this.
- **Effort**: small (audit only) — implementation is follow-up tasks
- **Model**: opus (design-heavy)
- **Parallel-safe**: yes (audit, not edit)
- **Depends on**: none
- **Deliverable**: ADR-0011 + `docs/perf/observability-audit-2026-05-18.md` listing every span to add. Commit stub: `observability: ADR-0011 contract + audit of un-spanned hot paths`.
- **Risk if skipped**: M10.5 iOS debugging is `print` archaeology; production support is impossible.

### 12. Property tests for composite-reverse-index reactivity (proptest)

- **Category**: correctness / testing
- **Scope**: Add `crates/nmp-core/tests/reverse_index_proptest.rs` with proptest strategies for (a) interest-shape insertion order is irrelevant to final state; (b) `rev` monotonically increases across all dispatches; (c) inserting then removing N events leaves the index in the empty-state; (d) coalescer emits one update per view per tick regardless of input cardinality.
- **Why now**: D8 reactivity invariants are asserted in bench but not property-tested. A proptest finding a counter-example is the difference between "we think it holds" and "we know it holds."
- **Effort**: small-medium
- **Model**: opus (invariant identification is non-trivial)
- **Parallel-safe**: yes (new test file)
- **Depends on**: none
- **Deliverable**: 1 file ≤200 LOC + `cargo test -p nmp-core reverse_index_proptest` green + each property documented in module rust-doc. Commit stub: `correctness(reverse-index): proptest invariants for D8 reactivity`.
- **Risk if skipped**: D8 is a doctrine without a proof; a regression slips through unit tests.

### 13. Examples directory with runnable mini-apps

- **Category**: DX / docs
- **Scope**: `examples/` at repo root with `hello-nostr` (read-only feed of 100 events from one relay) + `publish-note` (compose + sign + outbox publish, requires nsec env var) + `profile-viewer` (fetch and render a single npub). Each ≤150 LOC, compiles + runs via `cargo run --example`.
- **Why now**: there is *no* `cargo run --example anything` in this repo today. Every onboarding agent (human or LLM) currently learns from tests. Examples are 5× more discoverable.
- **Effort**: small-medium
- **Model**: sonnet
- **Parallel-safe**: yes (new top-level dir)
- **Depends on**: none (uses current in-memory store + raw kernel)
- **Deliverable**: 3 examples + `examples/README.md` index + 1 line in root `README.md` pointing to them. Commit stub: `dx(examples): 3 runnable mini-apps (hello-nostr, publish-note, profile-viewer)`.
- **Risk if skipped**: onboarding friction; LLM agents can't grok the kernel surface from tests alone.

### 14. Codex post-merge review tooling extraction

- **Category**: tooling / self-improvement
- **Scope**: Extract the codex-review invocation from ad-hoc orchestrator shell into `scripts/codex-review.sh <sha>` taking a SHA and emitting `docs/perf/codex-reviews/<sha>.md`. Document the heredoc + `< /dev/null` pattern (from `reference-codex-exec.md`). Add `make codex-review-last` shortcut.
- **Why now**: the brief identifies post-merge codex review as a per-commit ritual. Today it's repeated bespoke shell. A script makes it 1-command + fixes the stdin-hang footgun in memory.
- **Effort**: small (1-2h)
- **Model**: sonnet
- **Parallel-safe**: yes (new script)
- **Depends on**: none
- **Deliverable**: `scripts/codex-review.sh` + `Makefile` target + 5-line `README` note. Commit stub: `tooling(codex-review): extract per-merge review into scripts/codex-review.sh`.
- **Risk if skipped**: codex-review ritual is fragile shell each time; new agents reinvent the heredoc footgun.

### 15. PD-002 / PD-003 / PD-004 spec-gap reconciliation

- **Category**: spec-gap / self-improvement
- **Scope**: The brief lists PD-002/003/004 as open. `pending-user-decisions.md` shows only PD-001 (resolved). Either (a) those PDs were never logged and the brief is hallucinating, or (b) they were logged elsewhere. This task reconciles: search the repo + orchestration log + commit messages for any "PD-002" / "PD-003" / "PD-004" reference, document the finding, and either remove the brief's claim or surface the actual decisions in `pending-user-decisions.md`.
- **Why now**: open decisions cannot block work that doesn't know they exist; closed-but-undocumented decisions accumulate as future debt.
- **Effort**: small (1h)
- **Model**: sonnet
- **Parallel-safe**: yes
- **Depends on**: none
- **Deliverable**: update to `docs/perf/pending-user-decisions.md` (or note saying "no PD-002+ entries found anywhere — brief is incorrect"). Commit stub: `perf(pending-decisions): reconcile PD-002/003/004 brief claim with live ledger`.
- **Risk if skipped**: orchestrator decisions made on phantom PDs; user surprised when missing decisions surface.

---

## TIER 2 — queue after Tier-1 + e2e iOS lands (28)

Compact-row format. **Cat** abbreviations: perf=performance, corr=correctness, sec=security, obs=observability, doc=docs, test=testing, ci=CI, ref=refactor, NIP=NIP coverage, app=app breadth, xp=cross-platform, dx=DX, soak=soak, comp=compliance, ios=iOS, db=database, net=networking, mig=migration, refimp=reference impl, self=self-improvement, gap=spec-gap, tool=tooling.

| # | Title | Cat | Effort | Model | Parallel-safe | Deliverable (≤1 sentence) |
|---|---|---|---|---|---|---|
| 16 | NIP-50 search-relay query builder + integration tests | NIP | M | sonnet | yes | `nmp-core::search` filter shape + 6 tests against the public `search.nos.lol` relay. |
| 17 | NIP-51 mute-list + bookmark-list domain module | NIP | M | sonnet | yes | `nmp-core::lists` module + 4 tests; treats lists as first-class facts (D4). |
| 18 | NIP-23 long-form article module | NIP | M | sonnet | yes | kind:30023 ingest + render-spec + replaceable-event invariant tests. |
| 19 | kind:6 repost + kind:7 reaction modules | NIP | S | sonnet | yes | two modules + tests; surfaces the M7 interaction-loop dependency early. |
| 20 | NIP-92 inline-media parser | NIP | S | sonnet | yes | tag → media-descriptor decoder + tests; precondition for blossom UI. |
| 21 | Reactivity-bench p50/p95/p99 emission | perf | S | sonnet | yes | extend `report.rs` to emit percentiles per scenario + commit a baseline JSON. |
| 22 | Firehose-bench p50/p95/p99 + per-relay breakdown | perf | M | sonnet | coordinate-with-T58 | extend `report.rs`; include per-relay ingest latency. |
| 23 | Memory profiling pass (instruments + dhat) | perf | M | opus | yes | `docs/perf/memory-2026-05-18.md`: peak/avg/leak per scenario; allocations attributed. |
| 24 | Allocation-count gate for hot paths | perf | M | opus | yes | wrap key paths in `dhat-rs` test; assert ≤N allocations after warmup (D8 enforcement). |
| 25 | cargo-mutants mutation testing baseline | test | M | sonnet | yes | run on `nmp-core`; commit kill-ratio baseline + first-pass tests for surviving mutants. |
| 26 | cargo-fuzz harness for event parser | sec/corr | S | sonnet | yes | `fuzz/fuzz_targets/event_parse.rs` + 1h corpus seed + crash-corpus committed. |
| 27 | cargo-fuzz harness for NIP-19 (after #3) | sec/corr | S | sonnet | depends-on-#3 | round-trip fuzz; finds malformed bech32 panics before users do. |
| 28 | Tracing-span implementation (post #11 audit) | obs | M | sonnet | depends-on-#11 | add the spans `#11` audited; emit JSON via `tracing-subscriber`. |
| 29 | iOS Instruments leaks instrumentation script | ios | S | sonnet | yes | `scripts/ios-leaks.sh` automating Instruments → JSON for CI consumption. |
| 30 | iOS battery profile harness | ios | M | opus | yes | repeatable battery-drain test against `NmpStress`; baseline ledger. |
| 31 | Android cargo-ndk build verification | xp | M | sonnet | yes | green `cargo ndk -t arm64-v8a -t x86_64 build` + CI step. |
| 32 | wasm32 build smoke (no relay yet) | xp | S | sonnet | yes | `cargo build -p nmp-core --target wasm32-unknown-unknown` + IndexedDB-store stub. |
| 33 | Desktop iced/egui shell skeleton (read-only feed) | xp/refimp | L | opus | yes | minimal binary that runs the kernel + renders 100 events in a list. |
| 34 | LMDB write-amplification audit (when M3 lands) | db | M | opus | depends-on-M3-impl | measure logical bytes vs on-disk bytes per insert pattern; ADR with mitigation. |
| 35 | EventStore corruption-recovery tests | db | M | opus | depends-on-M3-impl | inject torn writes; assert no UB + crash-recovery semantics documented. |
| 36 | WebSocket reconnect-storm protection | net | M | opus | yes | exponential backoff w/ jitter + cap; replay corpus shows ≤N reconnects in 60s. |
| 37 | Relay reputation scoring substrate | net | M | opus | yes | per-relay latency + uptime + success-rate ledger; feeds future outbox selection. |
| 38 | IPv6 verification across relay client | net | S | sonnet | yes | force `dns_resolver` to v6 + run real-relay smoke; tag relays that fail. |
| 39 | TLS pinning option in relay client | sec | S | opus | yes | feature-flagged TLS pin per relay URL + ADR justifying the trade-off. |
| 40 | Applesauce → NMP migration guide | doc/mig | S | opus | coordinate-with-T59 | 10-pattern side-by-side; mirror of #9. |
| 41 | Profile-viewer-only minimal app | app/refimp | M | sonnet | yes | `apps/profile-viewer/` — npub-in, profile-out, no compose/no react; ~250 LOC. |
| 42 | Publish-only minimal app | app/refimp | M | sonnet | yes | `apps/publisher/` — sign + outbox publish a kind:1 from a nsec; smoke test. |
| 43 | NIP-29 chat client minimal app | app/refimp | L | opus | depends-on-T56 | `apps/chat29/` — once T56's nmp-nip29 crate is wirable; smoke test against `groups.0xchat.com`. |
| 44 | Replay-corpus runner against M2 plan compiler | test/perf | M | sonnet | depends-on-#2 + T58 | feed corpus #2 through the planner; assert no spurious plan-id churn. |

---

## TIER 3 — nice-to-have / future / exploratory (18)

| # | Title | Cat | Effort | Model | Notes |
|---|---|---|---|---|---|
| 45 | Differential testing vs nostr-rs-relay | corr | L | opus | spin up nostr-rs-relay in docker; compare frame-by-frame against NMP's expectations. |
| 46 | NIP-01 conformance matrix doc | comp | M | sonnet | every MUST/SHOULD/MAY → cell with implementation status + test pointer. |
| 47 | Formal D8 reactivity rev-monotonicity proof (TLA+ or Kani) | comp | L | opus | exploratory; gate on whether Kani can model the actor loop. |
| 48 | Reproducible-build verification | sec | M | opus | bit-for-bit reproducible across two machines; ADR on the trade-offs. |
| 49 | SAFETY-comment audit pass #2 (post-M10.5) | sec | M | opus | re-audit after M10.5 FFI grows; assert every `unsafe` block has accurate SAFETY. |
| 50 | Tor onion-relay support smoke test | net | M | opus | wire SOCKS5 proxy; smoke against `wss://*.onion`. |
| 51 | iCloud / Files-app key-export flow | ios | M | opus | export nsec encrypted to iCloud; recoverable across devices. |
| 52 | APNS push notifications (research-only) | ios | M | opus | which kinds map to notif? what's the relay-to-APNS gateway shape? |
| 53 | Lock-screen widgets | ios | L | opus | depends on WidgetKit + reactivity model survives extension cap; exploratory. |
| 54 | Share-extension target wiring | ios | L | opus | NIP-21 URL-in → publish-out; depends on #4. |
| 55 | Multi-account-switcher reference app | app/refimp | L | opus | depends on M8; exercises the substrate end-to-end. |
| 56 | Damus-clone minimal app | app/refimp | XL | opus | stress test; only after M2/M3/M4/M5/M6/M7/M8 actually land in code. |
| 57 | Glossary doc | doc | S | sonnet | every term used in docs cross-referenced; reduces onboarding friction. |
| 58 | NIP-by-NIP coverage matrix | comp/doc | M | sonnet | one row per NIP × column for impl/test/doc/example status. |
| 59 | 24-hour soak test (real relay, replay corpus) | soak | L | opus | overnight; emits a single-page report. |
| 60 | Backup/restore CLI for LMDB store | db/tool | M | opus | snapshot-export + import; non-trivial when paired with M8 multi-session. |
| 61 | nmp-cli scaffolding `nmp new my-app` | dx/tool | L | opus | M16 scope; can start the spec-only pass earlier. |
| 62 | Failure-mode catalog (every way the parallel-agent system has gone wrong) | self | M | opus | mine `orchestration-log.md` + codex reviews + commit history for every miss; one-page mitigations doc. |

---

## Meta-suggestions for the orchestration system itself (5)

1. **Single source of truth for in-flight tasks.** The brief references T56–T60 but `pending-user-decisions.md` and `orchestration-log.md` carry different ledgers, and the TaskList isn't visible to spawned agents. A single `docs/perf/inflight.json` (or equivalent) atomically updated on dispatch and on completion would let every fan-out agent know exactly what's in motion without prose archaeology. Bonus: the orchestrator's heartbeat could lint this file for staleness.

2. **Worktree → task affinity in the agent prompt.** Spawned agents currently learn which worktree they're in only from `cwd`. If the agent prompt prepended a `## You are: T## <name>` block + `## Touched-by-active-work files: [...]`, the agent could pre-empt collisions without re-reading the entire orchestration log. Cuts orientation tokens by ~40%.

3. **Codex-review failure → automatic fix-it task creation.** Today every codex review surfaces 4–10 issues that become hand-crafted `T##-codex-fixer-N` dispatches. A small parser that ingests `docs/perf/codex-reviews/<sha>.md` and emits a structured task list (per-issue: severity, file, suggested-fix) would let the orchestrator dispatch fixers without prose re-reading. Reduces orchestrator cognitive load per heartbeat.

4. **Promote `agent-push-protocol.md` to a pre-commit hook.** Memory says "agents in worktrees MUST use `git push origin HEAD:master`". Currently enforced by human (orchestrator) inspection at heartbeat. A worktree-aware pre-push hook that rewrites `git push origin master` → `git push origin HEAD:master` (or aborts with a helpful error) makes the protocol self-enforcing.

5. **Brief–reality drift detector.** This brief described a state (HB27 onwards, M5–M8 substrate landed) that the live tree doesn't reflect. A 50-LOC linter that diffs the brief's claimed state against `docs/plan/status.md` + `orchestration-log.md` head, and surfaces the deltas in the first 100 tokens of the agent's response, would prevent every spawned agent from doing this reconciliation independently. The brief should be a *prompt*, not a *spec*; reality is in the repo.

---

*Authored 2026-05-18 by parallel-work-brainstorm-2026-05-18 worktree agent (T61). Single-commit, push via `git push origin HEAD:master`. Post-merge codex review per `~/.claude/projects/.../post-merge-codex-review.md` is the orchestrator's responsibility, not this agent's.*
