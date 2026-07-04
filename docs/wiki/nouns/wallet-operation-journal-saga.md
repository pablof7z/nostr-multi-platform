---
type: noun-entry
slug: wallet-operation-journal-saga
name: "wallet operation journal (saga)"
origin: extracted
source_refs:
  - transcript:407-407
  - transcript:454-456
  - transcript:568-579
---

# wallet operation journal (saga)

A durable write-side saga (Draft→Prepared→MintPending→MintSettled→PublishPending→Settled/Unknown/Failed) whose entire purpose is at-most-once money safety — surviving process death after an irreversible external effect such as a mint spend
