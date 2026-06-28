# Chirp Feature Test Checklist (iOS / Android / TUI)

Chirp Web readiness is tracked separately in GitHub issue #2038 and
`docs/product-spec/chirp-web.md`. This checklist is the native/TUI parity
surface; do not use it as the web product-readiness authority.

Legend: ✅ expected · ⚠️ partial/known-gap. Columns: **iOS · Android · TUI**.

## Startup
- [ ] App launches, no crash, splash → Home. ✅·✅·✅
- [ ] Feed populates within ~5s from real relays (notes, not empty). ✅·✅·✅

## Home Timeline
- [ ] Notes render in reverse-chron order. ✅·✅·✅
- [ ] Avatars load (image, not placeholder for known pubkeys). ✅·✅·⚠️(text)
- [ ] Display names show (not raw npub/hex). ✅·✅·✅
- [ ] Relative timestamps ("2h", "now"). ✅·✅·✅
- [ ] Pull-to-refresh / scroll loads more. ✅·✅·⚠️

## Note Content
- [ ] Plain text wraps correctly. ✅·✅·✅
- [ ] URLs are tappable links. ✅·✅·⚠️
- [ ] Inline images render. ✅·✅·❌
- [ ] Video embeds play / show thumbnail. ✅·⚠️·❌
- [ ] #hashtags styled & tappable. ✅·✅·⚠️
- [ ] @mentions resolve to display name (not npub). ✅·✅·⚠️

## Profile
- [ ] Tap avatar/name → profile screen. ✅·✅·⚠️
- [ ] Bio, follower/following counts, user's posts list. ✅·✅·⚠️

## Social Actions
- [ ] Follow → button flips to Following; unfollow reverts (kind-3). ✅·✅·⚠️
- [ ] Reply: compose sheet, sends kind-1 with `e`/`p` tags; appears in thread. ✅·✅·✅
- [ ] React/like: tap heart → count increments (kind-7). ✅·✅·✅
- [ ] Repost (kind-6) / quote (kind-1 with `q`). ✅·⚠️·⚠️
- [ ] Compose new note → publishes, appears in own feed. ✅·✅·✅

## Discovery
- [ ] Search by name/npub/hashtag returns results. ⚠️·⚠️·❌

## Thread
- [ ] Tap note → thread view with parent + replies nested. ✅·✅·✅

## Notifications
- [ ] Mentions/replies/reactions surface in a notifications view. ⚠️·⚠️·❌

## DMs / Chats
- [ ] Chats tab lists conversations; open thread; send/receive (NIP-17). ✅·✅·⚠️

## Groups (NIP-29)
- [ ] Groups tab lists joined groups; open, read, post messages. ✅·✅·⚠️

## Marmot (MLS)
- [ ] Encrypted group: create/join, send/receive decrypts correctly. ✅·⚠️·❌

## Wallet / Zaps
- [ ] Wallet tab shows balance (NWC). ✅·⚠️·❌
- [ ] Zap a note → invoice pays, zap count increments. ✅·⚠️·❌

## Settings
- [ ] Relay list view: add/remove relay, status indicators. ✅·✅·⚠️
- [ ] Account switching / multi-account. ✅·✅·⚠️
- [ ] Sign out / sign in (nsec, NIP-46 bunker). ✅·✅·✅

## Parity Notes
- Same nmp-core kernel: a published note/reaction on iOS must appear on Android & TUI after relay roundtrip.
- TUI is read-leaning: media, wallet, search, notifications are display-limited.
- Mentions must resolve via claimed_profiles/mention_profiles (no raw npub leakage).
