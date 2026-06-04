---
title: NMP Type Safety & Identity Type Aliases
slug: nmp-type-safety-identity-types
summary: String-aliased identity types (such as IdentityId, Pubkey, RelayUrl, PublishHandle, and ActionId, which are all defined as equivalent to String) compromise type
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-03
updated: 2026-06-03
verified: 2026-06-03
compiled-from: conversation
sources:
  - session:b4fe9cec-eb86-47f7-bc1d-3c28a18d5fcf
---

# NMP Type Safety & Identity Type Aliases

## Type-Safety of String-Aliased Identity Types

String-aliased identity types (such as IdentityId, Pubkey, RelayUrl, PublishHandle, and ActionId, which are all defined as equivalent to String) compromise type safety by allowing silent mixing at compile time. [^b4fe9-9]

## See Also

