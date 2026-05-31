---
title: Chirp Profile Card
slug: chirp-profile-card
summary: ProfileCard includes a has_profile boolean that gates display of the about and nip05 fields, preventing debug placeholder text from appearing as bio.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-05-21
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:19e076ce-1291-4c21-80a6-950623f0d9b8
---

# Chirp Profile Card

## ProfileCard

ProfileCard includes a has_profile boolean that gates display of the about and nip05 fields, preventing debug placeholder text from appearing as bio. [^19e07-12]


## AccountSummary

AccountSummary includes a picture_url field enriched from kind:0 profile data via accounts_enriched(), so the home feed toolbar and compose sheet show the user's real profile picture. [^19e07-13]

## AccountsView

AccountsView displays the user's real profile picture using account.pictureUrl instead of a nil placeholder. [^19e07-14]
## See Also

