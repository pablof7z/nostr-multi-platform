---
title: Dispatcher Shutdown
slug: dispatcher-shutdown
topic: concurrency
summary: The dispatcher thread is joined in `cancel()` with a race guard to prevent thread leaks
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
---

# Dispatcher Shutdown

## Dispatcher Shutdown

The dispatcher thread is joined in `cancel()` with a race guard to prevent thread leaks. The network/broker concurrency fix replaced a mutex held across `translate` with lock-free `prepare_event` + O(1) `apply_prepared`. PR #1279 also fixes the mutex-held-across-translate defect pinned by a structural guard test. Additionally, PR #1279 fixes the ~75s shutdown block by adding a 10s `TcpStream::connect_timeout` and `client_tls_with_config`, tested against an RFC 5737 black-hole. Finally, it fixes Relaxed ordering on ARM by switching all 6 cancel-flag sites to Release stores + Acquire loads.

<!-- citations: [^02745-6] [^02745-54] -->
