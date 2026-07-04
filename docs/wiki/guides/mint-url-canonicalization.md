---
title: Mint URL Canonicalization
slug: mint-url-canonicalization
topic: wallet-architecture
summary: The `canonicalize_mint_url` function lowercases only the scheme and authority (split at the first of `/`, `?`, or `#`), strips exactly one trailing slash from t
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-07-04
updated: 2026-07-04
verified: 2026-07-04
compiled-from: conversation
sources:
  - session:91a86fdf-624c-446e-9b38-0fb02085121f
---

# Mint URL Canonicalization

## `canonicalize_mint_url` Function

The `canonicalize_mint_url` function lowercases only the scheme and authority (split at the first of `/`, `?`, or `#`), strips exactly one trailing slash from the path portion, and preserves path case, query strings, and fragments. <!-- [^91a86-d5965] -->

Mint URL canonicalization is applied at proof storage (`add_proofs`), proof lookup (`select_proofs`), deposit accepted-mint check, send mint resolution, redeem accepted-mint check, and all ledger `TokenAdded` fact mint keys. <!-- [^91a86-1cdf3] -->

The outgoing nutzap `u` tag uses the recipient's raw mint URL string, never the canonicalized form — canonicalization is only for comparison. <!-- [^91a86-d7f65] -->
