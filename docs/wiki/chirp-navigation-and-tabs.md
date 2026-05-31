---
title: Chirp Navigation and Tabs
slug: chirp-navigation-and-tabs
summary: The bottom tab bar contains five tabs
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-19
updated: 2026-05-25
verified: 2026-05-19
compiled-from: conversation
sources:
  - session:cb3376a7-cea1-49ac-b6dd-9251fa1af14a
  - session:eb342a0d-84e3-4289-9873-88a947ca8144
  - session:93c599f0-3aea-440a-9c42-1de6cd8771fe
---

# Chirp Navigation and Tabs

## Bottom Tab Bar

The bottom tab bar contains five tabs: Home, Chats, Groups, Wallet, and Settings. Tab badges appear on the tab bar showing unread counts, connection status, and attention indicators (e.g., [1 Home •3] [2 Chats •2] [3 Groups] [4 Wallet ●] [5 Settings ⚠]). The title bar shows the active account name, tab labels with badges, relay connection dot, and relay count. Search is moved to a toolbar button on Home rather than being a tab. DMs and groups are separated into distinct tabs rather than mixed in a single view. The Chats tab uses the icon bubble.left.and.bubble.right.fill; the Groups tab uses the icon person.3.fill. A "Channels" tab is not added to separate NIP-29 from Marmot groups because public vs. private is a row property, not a navigation axis. The Activity tab is removed from the bottom TabView bar.

<!-- citations: [^cb337-1] [^eb342-5] [^93c59-4] -->
## Activity Access

Activity is accessible via a bell toolbar button positioned in the top-right toolbar, left of the compose button. Tapping the bell toolbar button presents NotificationsView in a NavigationStack sheet. [^cb337-2]

## Chats Tab

The DM tab navigation title is "Chats" rather than "Messages". [^eb342-6]

## Tab Layouts

All Chats, Groups, Wallet, and Settings tabs have proper 2-pane approach-b layouts with colored cards, status dots, and real navigation, with no :command hint text anywhere. [^93c59-5]
## See Also

