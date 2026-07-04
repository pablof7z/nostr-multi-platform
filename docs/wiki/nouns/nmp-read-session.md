---
type: noun-entry
slug: nmp-read-session
name: "nmp-read-session"
origin: extracted
source_refs:
  - transcript:1115-1117
---

# nmp-read-session

A new Layer-4 crate that owns the single implementation of the read-lifecycle mechanics: ReadSessionRegistry (handle alloc + open/close + reverse teardown + one leak audit), open_read/close_read (replay-before-live, exact demand withdrawal, reverse teardown, typed-output tombstone), and the ReadHost seam. Required as a separate crate so the dependency arrow runs concept → engine ← runtime.
