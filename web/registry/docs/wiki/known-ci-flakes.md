---
title: Known CI Flaky Tests
slug: known-ci-flakes
summary: "The `relay_worker::tests::v58_set_backoff_hint_does_not_break_reconnect` test is a known flaky test (timing race on reconnect drain) that fails intermittently i"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-31
updated: 2026-05-31
verified: 2026-05-31
compiled-from: conversation
sources:
  - session:6a951af3-7b08-4d8d-adfd-361609270d50
---

# Known CI Flaky Tests

## Known CI Flakes

The `relay_worker::tests::v58_set_backoff_hint_does_not_break_reconnect` test is a known flaky test (timing race on reconnect drain) that fails intermittently in CI. It should be rerun rather than chased when a PR touches no Rust/nmp-network code. [^6a951-22]

## See Also

