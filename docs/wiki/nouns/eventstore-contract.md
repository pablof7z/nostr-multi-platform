---
type: noun-entry
slug: eventstore-contract
name: "EventStore contract"
origin: extracted
source_refs:
  - transcript:149-152
---

# EventStore contract

synchronous, &self (interior mutability), Send + Sync; every method is synchronous; scans return Box<dyn EventIter + 'a> requiring materialized owned rows due to Send bound
