---
type: noun-entry
slug: walletfact
name: "WalletFact"
origin: extracted
source_refs:
  - transcript:516-527
  - transcript:529-532
---

# WalletFact

A post-observation typed fact in the event-sourced wallet reducer stream; each carries a WHY and provenance. Variants include TokenAdded, TokenDeleted, MintProbed, NutzapRedeemed, SagaTransition, and StateRebuilt. Folded two ways: a bounded time-ordered delta ring (causal trail) and a per-atom last-cause index.
