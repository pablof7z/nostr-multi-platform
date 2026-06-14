---
title: Keystone Overview
slug: keystone-overview
topic: codebase-patterns
summary: "The three keystones that discharge most of the six patterns are: K1 (signer-session port covering sign/encrypt/decrypt with mailbox-completion delivery), K2 (in"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-14
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
---

# Keystone Overview

## Keystones

The three keystones that discharge most of the six patterns are: K1 (signer-session port covering sign/encrypt/decrypt with mailbox-completion delivery), K2 (instance-scoped registration eliminating OnceLock globals and kernel_mut), and K3 (coverage ledger replacing the presence heuristic). The three keystones K1, K2, K3 are executed sequentially (K1 first, then K2, then K3), with each fully landed on master before the next begins. All three keystones are complete and active on master: K1 signer-session port (ADR-0050), K2 instance-scoped registration (ADR-0052), K3 coverage ledger (ADR-0056). K1 completed via ADR-0050 (#1198), waking actor inbox (#1221), three-verb port + unified park/drain (#1242), gift-wrap chained through port + SignerForSeal deleted (#1255), gift-unwrap through port + raw-Keys slot deleted (#1258), bounded bunker decrypt queue + decrypt_state projection (#1322). K2 completed via ADR-0052 (#1323), register-by-value + ACTIVE_WALLET_RUNTIME deleted (#1326), per-app bunker/NIP-55 ports with all four hook/driver globals deleted (#1344), DispatchHostOp merged into Protocol (#1356), kernel_mut deleted (#1363), D21 no-ambient-authority lint (#1369). Of 11 needs-decision issues, 10 were determined by documented product direction; only #1281 required a genuine owner product-contract choice. The two-instance interop test (two NmpApp instances with separate wallets, separate bunker sessions, separate everything, no crosstalk) passes and was the oracle for K2 rung 5.2.

<!-- citations: [^2e544-412] [^2e544-61] [^02745-131] [^2e544-372] [^2e544-393] [^2e544-411] [^2e544-432] -->
## 30-Day Call

The 30-day call is: K1 through gift-unwrap (6.4), K2 through the global-hook slots (5.3), and the sync-soundness pair (un-floored NEG-OPEN + slot-lifetime cache-serve marker) — plus the durable money boundary dispatched in parallel.

<!-- citations: [^2e544-62] [^2e544-373] -->

## Breaking Changes — nmp-v0.7.0

The six breaking changes in nmp-v0.7.0 are: (a) ActionModule is register-by-value with &self methods, (b) signer-session port replaces SignerForSeal and per-crate raw-Keys slots, (c) kernel_mut() removed, (d) DispatchHostOp merged into Protocol, (e) the five process-global hooks/runtimes are now per-app ports, (f) coverage-ledger floor is active by default. nmp-v0.7.0 is the BREAKING keystone release and nmp-v0.7.2 is the consumer-ready release (with blossom restored as a v1 member and parked crates standalone-buildable). (Previously: nmp-v0.7.0 is the breaking keystone-series release and nmp-v0.7.1 fixes a parked-crate standalone-buildability defect; consumers should pin to 0.7.1 or later.)

<!-- citations: [^2e544-394] [^2e544-413] [^2e544-433] -->
