# ADR-0023 — HTTP Work Runs Off The Actor

- **Status:** Accepted
- **Date:** 2026-05-21

## Decision

HTTP and other blocking I/O do not run on the actor thread.

Network work is owned by the crate that understands the protocol. For example,
NIP-57 LNURL HTTP runs in `nmp-nip57` on bounded worker infrastructure, then
reports typed results back to Rust-owned action/projection state.

The capability socket remains for live native capability families where the
host must execute an OS-specific operation and return a fact. The host reports;
Rust decides.

## Consequences

- No actor message may synchronously wait on HTTP.
- Protocol crates own protocol-specific HTTP policy and parsing.
- Native capabilities remain fact-reporting bridges, not policy engines.
