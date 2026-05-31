---
title: NMP Desktop Avatar Rendering & Handle Management
slug: nmp-desktop-avatar-rendering
summary: "UserAvatar renders the actual profile picture from picture_url once kind:0 data arrives from relays, falling back to an initials circle on first load"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-28
updated: 2026-05-28
verified: 2026-05-28
compiled-from: conversation
sources:
  - session:6e8af009-f065-464a-98f1-3ec1ee4ed933
---

# NMP Desktop Avatar Rendering & Handle Management

## Avatar Rendering and Display

UserAvatar renders the actual profile picture from picture_url once kind:0 data arrives from relays, falling back to an initials circle on first load. Avatar image bytes are fetched in a background thread so the UI never freezes. The avatar image Handle is created once when fetch bytes arrive and stored, then reused every render frame to prevent iced from re-uploading the texture each frame. UserCard internally uses UserAvatar and receives the avatar image handle via an avatar_handle builder, with GalleryApp threading the handle through. [^6e8af-4]

## See Also

