---
title: Signer Broker Handshake Loop
slug: signer-broker-handshake-loop
topic: ffi-runtime
summary: Signer broker handshake must use crossbeam-channel with select_biased on a one-shot cancel channel and deadline timer, eliminating the 200ms polling recv_timeou
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-18
updated: 2026-06-19
verified: 2026-06-18
compiled-from: conversation
sources:
  - session:019edc02-4996-7cc0-8470-8fd907d283e4
  - session:11850f79-923f-4a2a-a921-a4b9bec47c6c
---

# Signer Broker Handshake Loop

## Handshake Await Loop

Signer broker handshake must use crossbeam-channel with select_biased on a one-shot cancel channel and deadline timer, eliminating the 200ms polling recv_timeout loop. The handshake fix requires editing broker.rs, broker/nostrconnect.rs, broker/restore.rs, and Cargo.toml in addition to handshake.rs because a handshake.rs-only edit would not compile. The P5 handshake fix replaces the 200ms recv_timeout cancel-poll with an event-driven crossbeam select_biased!(cancel_rx, after(deadline), inbound_rx), removing the timer poll entirely (D8).

<!-- citations: [^11850-197] [^019ed-23] [^019ed-24] [^11850-59] [^11850-83] [^11850-123] [^11850-149] [^11850-196] [^11850-220] [^11850-232] [^11850-254] -->
## Inbound Channel

The inbound channel in nmp-signer-broker sessions must change from std::sync::mpsc::channel to crossbeam_channel::unbounded to support crossbeam select. <!-- [^019ed-25] -->

## Cancellation Mechanism

ActiveSession must include a cancel_tx: crossbeam_channel::Sender<()> created as a bounded(1) one-shot channel; cancel() must call try_send on it after the AtomicBool release store, making cancellation an event rather than a polled flag. The AtomicBool cancel flag must be retained for existing pre-await dial checkpoints but must not be used as the wait wakeup mechanism. <!-- [^019ed-26] -->

## Dependencies

nmp-signer-broker must add crossbeam-channel = "0.5" as a direct dependency. <!-- [^019ed-27] -->

## Sequencing

P5 handshake crossbeam migration (PR #1547) is sequenced to land before P9 PR1b (nostrconnect perms), because both edit broker/nostrconnect.rs. PR1b is sequenced LAST among P9's vertical, after p5 #1547 merges. PR3 (signer-labels-to-shells + P4 F3) proceeds in parallel with PR1b because it is independent of broker/nostrconnect.rs. (Previously: The p5 handshake fix must be sequenced after p9 PR1 because both lanes need broker/nostrconnect.rs.)

<!-- citations: [^11850-43] [^11850-61] [^11850-104] [^11850-124] [^11850-150] [^11850-174] [^11850-198] -->
## Stale Findings

The hung-spinner finding (no async success-terminal recording) is stale — success terminal was already moved to reconcile.rs by PR #1211 (durable tri-state NWC). <!-- [^11850-60] -->
