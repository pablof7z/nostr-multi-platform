---
title: Groups List UI
slug: groups-list-ui
summary: The groups list displays a single flat list sorted by last activity, with no section headers or protocol vocabulary visible to users
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-05-25
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:eb342a0d-84e3-4289-9873-88a947ca8144
  - session:93c599f0-3aea-440a-9c42-1de6cd8771fe
---

# Groups List UI

## Groups List Layout

DMs and groups are mixed in a single list with different indicators for encrypted vs. non-encrypted rather than grouped by type. NIP-29 groups show '#' in REPLY_COLOR blue, and Marmot MLS groups show 'E' in ZAP yellow, both in the same list. Public vs. private is a row-level property within the groups list, not a separate navigation axis; a 'Channels' tab must not be added to separate NIP-29 groups from encrypted groups.

<!-- citations: [^eb342-5] [^93c59-13] -->
## Pending Invites

Pending group invites appear as a collapsed chip at the top of the groups list, linking to a dedicated InvitesView. A red dot badge appears on the Groups tab icon to indicate pending invites. [^eb342-6]
## See Also

