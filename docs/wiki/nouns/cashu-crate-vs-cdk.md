---
type: noun-entry
slug: cashu-crate-vs-cdk
name: "cashu crate (vs CDK)"
origin: extracted
source_refs:
  - transcript:4641-4649
---

# cashu crate (vs CDK)

The modular audited `cashu` crate (pure BDHKE/DLEQ/P2PK primitives, zero I/O) — viable in NMP because its secp256k1 stack dedupes to the exact versions already in-tree via nostr; distinct from the async `cdk` wallet crate which bundles its own networking/storage and doesn't fit the substrate
