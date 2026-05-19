# Real-relay honest-validation reports

Single-page findings from the `real_relay_*` integration suite
(`crates/nmp-testing/tests/real_relay_*.rs`). Each scenario opens **real**
websockets to public Nostr relays to prove the kernel works end-to-end —
or to report *loudly* where it does not.

These tests are `#[ignore]`-gated (network) and run explicitly:

```bash
cargo test -p nmp-testing --test real_relay_connect  -- --ignored --nocapture
cargo test -p nmp-testing --test real_relay_outbox   -- --ignored --nocapture
cargo test -p nmp-testing --test real_relay_nip77    -- --ignored --nocapture
cargo test -p nmp-testing --test real_relay_nip42    -- --ignored --nocapture
cargo test -p nmp-testing --test real_relay_replan   -- --ignored --nocapture
NMP_SOAK_DURATION_SECS=120 \
  cargo test -p nmp-testing --test real_relay_soak    -- --ignored --nocapture
```

## Scenario map (spec §5 honest-validation gap)

| # | Scenario | Test file | Report stem |
|---|----------|-----------|-------------|
| 1 | connect + subscribe + receive a real third-party kind:1 | `real_relay_connect.rs` | `scenario1-connect` |
| 2 | NIP-65 outbox routing on a **real** author's kind:10002 | `real_relay_outbox.rs` | `scenario2-outbox` |
| 3 | NIP-77 negentropy sync vs REQ-fallback | `real_relay_nip77.rs` | `scenario3-nip77` |
| 4 | NIP-42 AUTH challenge/response on an auth-required relay | `real_relay_nip42.rs` | `scenario4-nip42` |
| 5 | publish a signed event + verify OK frame | `real_relay_smoke.rs::damus_round_trip_kind1` | (covered) |
| 6 | kind:3 follow-list change → subscription re-plan | `real_relay_replan.rs` | `scenario6-replan` |
| — | sustained multi-relay soak (leak / memory / panic) | `real_relay_soak.rs` | `soak-<unix-ts>` |

Scenario 5 is already proven by the pre-existing
`real_relay_smoke.rs::damus_round_trip_kind1` (publish → OK frame → REQ
echo). It is not duplicated.

## Reading a report

Every report carries YAML frontmatter and a `## Verdict: PASS|SKIP|FAIL`
line so the whole directory is greppable:

```bash
grep -r '^verdict:' docs/perf/real-relay/
```

A **SKIP** with a written finding is a legitimate, intended outcome — it
means a public relay did not exhibit the behaviour the scenario needs, and
that gap is now documented rather than hidden behind a fake green.
