---
type: noun-entry
slug: eventstoreslot
name: "EventStoreSlot"
origin: extracted
source_refs:
  - transcript:355-603
---

# EventStoreSlot

An Arc<Mutex<Option<Arc<dyn EventStore>>>> — the V-83 publish-back slot pattern that enables action modules to read the kernel's event store synchronously at execute time without requiring signature changes across all ActionModule implementations
