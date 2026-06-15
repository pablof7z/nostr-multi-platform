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
updated: 2026-06-15
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
  - session:019eca68-85c6-77e0-b237-e58f6c894f72
  - session:78b50727-bccd-4088-8493-a07624a4fa83
---

# Dispatcher Shutdown

## Dispatcher Shutdown

The dispatcher thread is joined in `cancel()` with a race guard to prevent thread leaks. BunkerBroker::cancel() is signal-only and returns immediately without joining worker threads; it sets the cancel flag, drains pending, shuts down the relay, and spawns a detached reaper thread for the joins, never blocking the caller path. A monotonic generation counter on BunkerBroker prevents stale workers from clobbering a freshly-staged session after cancel; install_session returns false unless the worker's generation stamp matches the active session's generation. DNS resolution for BunkerBroker connects runs on a detached helper thread with a TCP_CONNECT_TIMEOUT deadline; the worker abandons the helper on timeout and exits, bounding the stuck-DNS case, while a rare TLS-trickling-peer residual is documented as out of D4 scope. AppHost is still one ~30-method god-trait (needs splitting into narrow registration/capability traits). The network/broker concurrency fix replaced a mutex held across `translate` with lock-free `prepare_event` + O(1) `apply_prepared`. PR #1279 also fixes the mutex-held-across-translate defect pinned by a structural guard test. Additionally, PR #1279 fixes the ~75s shutdown block by adding a 10s `TcpStream::connect_timeout` and `client_tls_with_config`, tested against an RFC 5737 black-hole. Finally, it fixes Relaxed ordering on ARM by switching all 6 cancel-flag sites to Release stores + Acquire loads.

<!-- citations: [^02745-6] [^02745-54] [^019ec-10] [^019ec-45] [^78b50-197] [^78b50-202] [^78b50-230] [^78b50-237] -->
