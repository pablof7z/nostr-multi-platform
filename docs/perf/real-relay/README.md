# Real-Relay Verification Matrix

Real-relay verification is scheduled/manual CI, not a pull-request gate. PR
workflows keep hermetic fixture tests as the default signal; public relay tests
live in `.github/workflows/real-relay-nightly.yml` because they open live
WebSocket connections and depend on third-party relay capability.

The nightly workflow is an explicit matrix. Each row names one scenario,
declares its relay candidates, probes those relay candidates before running,
then records `PASS`, `SKIP`, or `FAIL` in the GitHub step summary and uploads
the captured output.

## Matrix

| Scenario | Capability | Relays | Command |
|---|---|---|---|
| `connect-subscribe` | connect/subscribe | `relay.damus.io`, `nos.lol`, `relay.primal.net` | `cargo test -p nmp-testing --test real_relay_connect -- --ignored --nocapture --test-threads=1` |
| `publish-ok` | publish OK | `relay.damus.io` | `cargo test -p nmp-testing --test real_relay_smoke damus_round_trip_kind1 -- --ignored --nocapture --test-threads=1` |
| `nip65-outbox` | NIP-65 outbox routing | `relay.damus.io`, `relay.primal.net`, `purplepag.es`, `nos.lol` | `cargo test -p nmp-testing --test real_relay_outbox -- --ignored --nocapture --test-threads=1` |
| `nip77-negentropy` | NIP-77 negentropy | `nip29.f7z.io` | `cargo run -p nmp-testing --bin nip77-real-relay-tui -- --once --neg-only --relay wss://nip29.f7z.io` |
| `nip42-auth` | NIP-42 auth | `nostr.wine`, `relay.snort.social`, `auth.nostr1.com`, `nostr.land`, `relay.damus.io` | `cargo test -p nmp-testing --test real_relay_nip42 -- --ignored --nocapture --test-threads=1` |
| `nip17-cold-start` | NIP-17 cold start | `relay.primal.net` | `cargo test -p nmp-testing --test real_relay_nip17_cold_start_kernel nip17_cold_start_receive_through_real_kernel -- --ignored --nocapture --test-threads=1` |
| `marmot-roundtrip` | Marmot roundtrip | `relay.damus.io` | `cargo test -p nmp-testing --test real_relay_marmot_roundtrip marmot_kind445_roundtrip_over_damus -- --ignored --nocapture --test-threads=1` |
| `feed-matrix` | declared feed matrix | `relay.damus.io`, `nos.lol`, `relay.primal.net`, `purplepag.es` | `cargo test -p nmp-testing --test real_relay_feed_matrix -- --ignored --nocapture --test-threads=1` |
| `subscription-replan` | follow-list replan | `relay.damus.io`, `relay.primal.net`, `purplepag.es` | `cargo test -p nmp-testing --test real_relay_replan -- --ignored --nocapture --test-threads=1` |
| `reduced-source-feed` | ReducedSource feed acquisition | `relay.damus.io`, `relay.primal.net`, `nos.lol` | `cargo test -p nmp-testing --test real_relay_reduced_source -- --ignored --nocapture --test-threads=1` |
| `nip50-search` | NIP-50 search relay | `nostr.wine`, `nos.lol` | `cargo test -p nmp-testing --features real-relay --test real_relay_nip50_search -- --ignored --nocapture --test-threads=1` |
| `relay-search-a1-cold-claim` | relay-search radius A1 | `relay.primal.net`, `purplepag.es` | `cargo test -p nmp-testing --features real-relay --test relay_search_radius_a1_cold_claim -- --ignored --nocapture --test-threads=1` |
| `relay-search-a2-warm-path` | relay-search radius A2 | `relay.primal.net`, `purplepag.es` | `cargo test -p nmp-testing --features real-relay --test relay_search_radius_a2_warm_path -- --ignored --nocapture --test-threads=1` |
| `relay-search-a3-lmdb-restart` | relay-search radius A3 LMDB | `relay.primal.net`, `purplepag.es` | `cargo test -p nmp-testing --features real-relay,lmdb-backend --test relay_search_radius_a3_restart_persistence -- --ignored --nocapture --test-threads=1` |
| `relay-search-a5-unreachable` | relay-search radius A5 | `relay.primal.net` | `cargo test -p nmp-testing --features real-relay --test relay_search_radius_a5_mid_claim_unreachable -- --ignored --nocapture --test-threads=1` |
| `relay-search-a6-concurrent` | relay-search radius A6 | `relay.primal.net`, `purplepag.es` | `cargo test -p nmp-testing --features real-relay --test relay_search_radius_a6_concurrent_claims -- --ignored --nocapture --test-threads=1` |

Rows run serially (`max-parallel: 1`) and every Cargo test row uses
`--test-threads=1`. That is intentional: some scenarios install global process
hooks or mutate public relay state, and parallel execution would make failures
harder to classify.

## Verdict Semantics

`PASS` means the row's command exited successfully and did not emit a scenario
`SKIP` marker.

`SKIP` means public relay conditions made the row non-falsifiable for this run.
The CI helper reports `SKIP` before Cargo when no relay candidate is reachable.
Several Rust tests can also emit `SKIP:` after connecting when a live relay does
not exhibit the capability the scenario needs, such as no usable kind:3 contact
list or no auth challenge. A skip is a recorded public-relay limitation, not a
product green.

`FAIL` means the command exited non-zero, or the NIP-77 binary reported a
negentropy error after the relay reachability probe passed. Treat failures as
product or harness regressions until triage proves otherwise.

## Local Runs

Run the same commands from the matrix when reproducing a row locally. Keep the
`--ignored`, feature flags, and `--test-threads=1` exactly as shown.

The older single-command pattern is intentionally retired:

```bash
cargo test -p nmp-testing -- --ignored --nocapture
```

That command hides which capability failed or skipped. The matrix rows are the
source of truth.

## Reports

The real-relay tests that write markdown findings still use this directory:

```bash
grep -r '^verdict:' docs/perf/real-relay/
```

Those report files are evidence snapshots. The scheduled/manual workflow
summary is the current CI signal.
