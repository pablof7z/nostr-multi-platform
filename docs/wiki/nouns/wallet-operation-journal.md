---
type: noun-entry
slug: wallet-operation-journal
name: "wallet operation journal"
origin: extracted
source_refs:
  - transcript:322-322
  - transcript:407-407
  - transcript:433-438
---

# wallet operation journal

A durable write-side saga with states Draft→MintPending→MintSettled→PublishPending→Settled (plus Unknown/Failed), persisted through NMP storage. Its defining requirement is surviving process death after an irreversible external effect (a mint spend) and reconciling against the mint as external authority to prevent double-spend.
