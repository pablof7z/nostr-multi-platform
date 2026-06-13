---
title: Network Pool Timeout
slug: network-pool-timeout
topic: concurrency
summary: "Network pool connection must use `TcpStream::connect_timeout` (10 s) and `client_tls_with_config` to prevent shutdown blocking for ~75 seconds on black-hole hos"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-13
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
---

# Network Pool Timeout

## Network Pool Timeout

Network pool connection must use `TcpStream::connect_timeout` (10 s) and `client_tls_with_config` to prevent shutdown blocking for ~75 seconds on black-hole hosts. The relayfail one-change fix makes permanent/transient a first-class end-to-end propagated state: classify on typed HTTP status, enter `Denied{until: Instant}` long-backoff, emit `PoolEvent::Closed{reason: Permanent}`, and let `ensure_open` respawn exited slots.

<!-- citations: [^02745-37] [^2e544-64] -->
