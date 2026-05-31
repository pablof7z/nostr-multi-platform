---
title: Chirp Group Invites & Creation
slug: chirp-group-invites-and-creation
summary: Pending group invites appear as a collapsed chip at the top of the groups list, linking to a dedicated InvitesView
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-05-27
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:eb342a0d-84e3-4289-9873-88a947ca8144
  - session:cd2b6122-2b7c-43fc-941b-c51e79ffc691
---

# Chirp Group Invites & Creation

## Pending Group Invites

Pending group invites appear as a collapsed chip at the top of the groups list, linking to a dedicated InvitesView. The Groups tab displays a red dot to indicate pending invites. [^eb342-2]



PendingGroupChange::drop silently clears unresolved MLS commits, causing group state to diverge from relay-published events. [^cd2b6-5]
## Group Creation

Group creation uses a Private/Public segmented toggle rather than separate creation concepts, defaulting to Private. Public is greyed out with a "Coming soon" label until fully supported. The member selector is a contact picker backed by the NIP-02 follow list, npub paste, and QR scan, instead of a raw npub textarea. [^eb342-3]
## See Also

