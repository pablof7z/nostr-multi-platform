---
type: noun-entry
slug: json-dispatch-helpers
name: "JSON dispatch helpers"
origin: extracted
source_refs:
  - transcript:1074-1088
---

# JSON dispatch helpers

Retired Android dispatch pattern where callers hand-assembled JSON and Rust
encoded it into FlatBuffers. M14-1 replaced this with generated FlatBuffers
action builders feeding the byte doorway, so production Android writes no longer
use JSON dispatch helpers.
