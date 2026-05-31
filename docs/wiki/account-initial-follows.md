---
title: Account Initial Default Follows
slug: account-initial-follows
summary: When a new account is created on Chirp, it automatically follows npub1l2vyh47mk2p0qlsku7hg0vn29faehy9hy34ygaclpn66ukqp3afqutajft and fiatjaf's key as its initia
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-19
updated: 2026-05-19
verified: 2026-05-19
compiled-from: conversation
sources:
  - session:f22be978-ccc6-42dd-bad0-2b2d5aba2999
---

# Account Initial Default Follows

## Account Initial Follows

When a new account is created on Chirp, it automatically follows npub1l2vyh47mk2p0qlsku7hg0vn29faehy9hy34ygaclpn66ukqp3afqutajft and fiatjaf's key as its initial contact list. A DEFAULT_FOLLOWS constant stores the two hex pubkeys for the default follows. A publish_initial_follows() helper builds a kind:3 contact list event and publishes it, and is called at the end of create_account(). [^f22be-1]

## See Also

