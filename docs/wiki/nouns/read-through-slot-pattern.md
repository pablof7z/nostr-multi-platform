---
type: noun-entry
slug: read-through-slot-pattern
name: "read-through-slot pattern"
origin: extracted
source_refs:
  - transcript:598-603
---

# read-through-slot pattern

The idiomatic pattern in nmp where stateful actions capture an EventStoreSlot at registration (ADR-0052) and read from it synchronously during execute(), avoiding changes to the ActionModule trait
