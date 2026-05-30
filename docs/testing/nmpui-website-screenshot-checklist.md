# nmpui.f7z.io Registry Screenshot Checklist

Generated: 2026-05-31. Audited against:
- `web/registry/src/registry/{content,user,relay,embeds}.ts`
- `web/registry/public/screenshots/` (actual disk listing)
- `apps/nmp-gallery/showcase-references.json` (canonical showcase identity)

Showcase constants (all platform galleries use the same references):
- **Profile**: pubkey `fa984bd7dbb282f07e16e7ae87b26a2a7b9b90b7246a44771f0cf5ae58018f52` (pablof7z)
  — npub_short "npub1l2vyh...utajft"
  — kernel-resolved display name: **"PABLOF7z"** (confirmed from `tui-embed-profile.txt` and `tui-embed-note.txt` TUI text dumps)
  — kernel-resolved NIP-05: NOT confirmed from any machine-readable source in this audit; do not hardcode a domain string in acceptance criteria — instead verify "domain-only format, no _@ prefix" and "a non-empty verified identifier"
- **Article**: Gigi's kind:30023 — title **"What's left of the internet?"**, author byline **"Gigi"** (both confirmed from `tui-embed-article.txt`)
- **Note**: pablof7z kind:1 note — content **"grok cli is INSANELY bad, jesus"**, author **"PABLOF7z"** (confirmed from `tui-embed-note.txt`)
- **Highlight**: pablof7z kind:9802 highlight (content only confirmed in PNG, not from txt dump — highlight txt shows empty content pane, meaning it was captured before relay resolved)
- **Relays**: wss://purplepag.es (indexer), wss://relay.primal.net (both+indexer)

---

## 1. Full Inventory Table

STATUS key: **OK** = file exists on disk | **DANGLING** = file referenced but missing | **EMPTY** = no screenshots array entries

### Section: Content

| Registry ID | Platform | Component | Referenced Screenshot(s) | File Exists? | STATUS |
|---|---|---|---|---|---|
| `swiftui-content-core` | SwiftUI | content-core | `content-core-ios-gallery-preview.png` | YES | OK |
| `compose-content-core` | Compose | content-core | `compose-content-core-preview.png` | YES | OK |
| `compose-content-core` | Compose | content-core | `content-core-kotlin-preview.png` | YES | OK |
| `tui-content-core` | TUI | content-core | `tui-content-core-preview.png` | YES | OK |
| `swiftui-content-minimal` | SwiftUI | content-minimal | `content-minimal-ios-gallery-preview.png` | YES | OK |
| `tui-content-minimal` | TUI | content-minimal | `tui-content-minimal-preview.png` | YES | OK |
| `swiftui-content-view` | SwiftUI | content-view | `content-view-ios-gallery-preview.png` | YES | OK |
| `compose-content-view` | Compose | content-view | `compose-content-view-preview.png` | YES | OK |
| `compose-content-view` | Compose | content-view | `content-view-kotlin-preview.png` | YES | OK |
| `tui-content-view` | TUI | content-view | `tui-content-view-preview.png` | YES | OK |
| `swiftui-content-kind-registry` | SwiftUI | content-kind-registry | `swiftui-content-kind-registry-preview.png` | YES | OK |
| `swiftui-content-kind-registry` | SwiftUI | content-kind-registry | `content-kind-registry-ios-gallery-preview.png` | **NO** | **DANGLING** |
| `tui-content-kind-registry` | TUI | content-kind-registry | `tui-content-view-preview.png` (shared) | YES | OK |
| `swiftui-content-kind-30023` | SwiftUI | content-kind-30023 | `swiftui-content-kind-30023-preview.png` | YES | OK |
| `swiftui-content-kind-30023` | SwiftUI | content-kind-30023 | `content-kind-30023-ios-gallery-preview.png` | **NO** | **DANGLING** |
| `tui-content-kind-30023` | TUI | content-kind-30023 | `tui-embed-article.png` | YES | OK |
| `swiftui-content-kind-9802` | SwiftUI | content-kind-9802 | `swiftui-content-kind-9802-preview.png` | YES | OK |
| `swiftui-content-kind-9802` | SwiftUI | content-kind-9802 | `content-kind-9802-ios-gallery-preview.png` | **NO** | **DANGLING** |
| `tui-content-kind-9802` | TUI | content-kind-9802 | `tui-embed-highlight.png` | YES | OK |
| `swiftui-content-mention-chip` | SwiftUI | content-mention-chip | `content-mention-chip-ios-gallery-preview.png` | YES | OK |
| `compose-content-mention-chip` | Compose | content-mention-chip | `compose-content-mention-chip-preview.png` | YES | OK |
| `compose-content-mention-chip` | Compose | content-mention-chip | `content-mention-chip-kotlin-preview.png` | YES | OK |
| `tui-content-mention-chip` | TUI | content-mention-chip | `tui-content-mention-chip-preview.png` | YES | OK |
| `swiftui-content-quote-card` | SwiftUI | content-quote-card | `content-quote-card-ios-gallery-preview.png` | YES | OK |
| `compose-content-quote-card` | Compose | content-quote-card | `compose-content-quote-card-preview.png` | YES | OK |
| `compose-content-quote-card` | Compose | content-quote-card | `content-quote-card-kotlin-preview.png` | YES | OK |
| `tui-content-quote-card` | TUI | content-quote-card | `tui-content-quote-card-preview.png` | YES | OK |
| `swiftui-content-media-grid` | SwiftUI | content-media-grid | `content-media-grid-ios-gallery-preview.png` | YES | OK |
| `compose-content-media-grid` | Compose | content-media-grid | `compose-content-media-grid-preview.png` | YES | OK |
| `compose-content-media-grid` | Compose | content-media-grid | `content-media-grid-kotlin-preview.png` | YES | OK |
| `tui-content-media-grid` | TUI | content-media-grid | `tui-content-media-grid-preview.png` | YES | OK |
| `swiftui-login-block` | SwiftUI | login-block | _(none)_ | — | **EMPTY** |

### Section: User

| Registry ID | Platform | Component | Referenced Screenshot(s) | File Exists? | STATUS |
|---|---|---|---|---|---|
| `tui-user-core` | TUI | user-core | _(none)_ | — | **EMPTY** |
| `swiftui-user-avatar` | SwiftUI | user-avatar | `user-avatar-ios-gallery-preview.png` | YES | OK |
| `compose-user-avatar` | Compose | user-avatar | `user-avatar-kotlin-preview.png` | YES | OK |
| `tui-user-avatar` | TUI | user-avatar | `tui-user-avatar-preview.png` | YES | OK |
| `swiftui-user-name` | SwiftUI | user-name | `user-name-ios-gallery-preview.png` | YES | OK |
| `compose-user-name` | Compose | user-name | `user-name-kotlin-preview.png` | YES | OK |
| `tui-user-name` | TUI | user-name | `tui-user-name-preview.png` | YES | OK |
| `swiftui-user-nip05` | SwiftUI | user-nip05 | `user-nip05-ios-gallery-preview.png` | YES | OK |
| `compose-user-nip05` | Compose | user-nip05 | `user-nip05-kotlin-preview.png` | YES | OK |
| `tui-user-nip05` | TUI | user-nip05 | `tui-user-nip05-preview.png` | YES | OK |
| `swiftui-user-npub` | SwiftUI | user-npub | `user-npub-ios-gallery-preview.png` | YES | OK |
| `compose-user-npub` | Compose | user-npub | `user-npub-kotlin-preview.png` | YES | OK |
| `tui-user-npub` | TUI | user-npub | `tui-user-npub-preview.png` | YES | OK |
| `swiftui-user-card` | SwiftUI | user-card | `user-card-ios-gallery-preview.png` | YES | OK |
| `compose-user-card` | Compose | user-card | `user-card-kotlin-preview.png` | YES | OK |
| `tui-user-card` | TUI | user-card | `tui-user-card-preview.png` | YES | OK |

### Section: Relay

| Registry ID | Platform | Component | Referenced Screenshot(s) | File Exists? | STATUS |
|---|---|---|---|---|---|
| `swiftui-relay-list` | SwiftUI | relay-list | `relay-list-ios-gallery-preview.png` | YES | OK |
| `swiftui-relay-list` | SwiftUI | relay-list | `tui-relay-list-preview.png` (shared) | YES | OK |

### Section: Embeds & Kinds
> Note: `embedComponents` in `embeds.ts` are rendered by the website (4 route pages) but do NOT appear in `registry.json` (the 41-item install manifest). Their screenshots are referenced from the website's route pages only. They map to the same install ids as `content-kind-30023`/`content-kind-9802` — they are the showcase faces of those components.

| Route Slug | Platform | Component | Referenced Screenshot(s) | File Exists? | STATUS |
|---|---|---|---|---|---|
| `embed-article` | SwiftUI | embed-article | `embed-article-ios-gallery-preview.png` | YES | OK |
| `embed-article` | TUI | embed-article | `tui-embed-article.png` | YES | OK |
| `embed-article` | TUI | embed-article | `tui-embed-article-preview.png` | YES | OK |
| `embed-profile` | SwiftUI (soon) | embed-profile | `embed-profile-ios-gallery-preview.png` | YES | OK |
| `embed-profile` | SwiftUI (soon) | embed-profile | `tui-embed-profile-preview.png` | YES | OK |
| `embed-note` | SwiftUI (soon) | embed-note | `embed-note-ios-gallery-preview.png` | YES | OK |
| `embed-note` | SwiftUI (soon) | embed-note | `tui-embed-note-preview.png` | YES | OK |
| `embed-highlight` | SwiftUI | embed-highlight | `embed-highlight-ios-gallery-preview.png` | YES | OK |
| `embed-highlight` | TUI | embed-highlight | `tui-embed-highlight.png` | YES | OK |
| `embed-highlight` | TUI | embed-highlight | `tui-embed-highlight-preview.png` | YES | OK |

---

## 2. Gap List

### DANGLING references (file referenced in TS but absent on disk)

Three registry entries each reference a second iOS gallery screenshot that does not exist:

#### 1. `swiftui-content-kind-registry` — `content-kind-registry-ios-gallery-preview.png`

**Does a suitable existing PNG exist under a different name?**
The only iOS gallery screenshot that covers the kind-registry is `swiftui-content-kind-registry-preview.png` (already referenced as the first screenshot). There is no separate "ios-gallery" variant on disk that could be aliased.
The closest captured iOS gallery screenshots for the pages that exercise `NostrKindRegistry` are:
- `embed-article-ios-gallery-preview.png` — shows an article embed resolved through the registry
- `embed-highlight-ios-gallery-preview.png` — shows a highlight embed resolved through the registry

Neither is a direct drop-in alias (they show specific kind renderers, not the registry dispatch mechanism itself).

**Action**: NEEDS CAPTURE — run the NmpGallery iOS simulator, navigate to the `content-kind-registry` showcase page, and capture as `content-kind-registry-ios-gallery-preview.png`. The page should show `EmbeddedEvent` resolving an event through `NostrKindRegistry` with the embed chrome (depth-graded accent stripe) visible.

---

#### 2. `swiftui-content-kind-30023` — `content-kind-30023-ios-gallery-preview.png`

**Does a suitable existing PNG exist under a different name?**
YES. `embed-article-ios-gallery-preview.png` IS the same screen: the gallery's `ArticleEmbedPage` renders a kind:30023 article through `ArticleEmbed`. The embed-article page IS the content-kind-30023 showcase page.

**Fix**: In `content.ts`, change the second screenshot reference from `content-kind-30023-ios-gallery-preview.png` to `embed-article-ios-gallery-preview.png`. This reference is an alias — the same captured frame serves both entries.

Alternatively: rename `embed-article-ios-gallery-preview.png` to `content-kind-30023-ios-gallery-preview.png` and update both `content.ts` and `embeds.ts`. Prefer the rename to keep naming consistent with the `content-kind-*` naming pattern.

---

#### 3. `swiftui-content-kind-9802` — `content-kind-9802-ios-gallery-preview.png`

**Does a suitable existing PNG exist under a different name?**
YES. `embed-highlight-ios-gallery-preview.png` IS the same screen: the gallery's `HighlightEmbedPage` renders a kind:9802 highlight through `HighlightEmbed`.

**Fix**: In `content.ts`, change the second screenshot reference from `content-kind-9802-ios-gallery-preview.png` to `embed-highlight-ios-gallery-preview.png`. Same alias argument as above.

Alternatively: rename to `content-kind-9802-ios-gallery-preview.png` and update both `content.ts` and `embeds.ts`.

---

### EMPTY entries (no screenshots at all)

#### 4. `swiftui-login-block` — screenshots: []

The login-block component has zero screenshots. There is no file on disk for it at all.

**Does a suitable file exist?** No.

**Action**: NEEDS CAPTURE — run NmpGallery iOS simulator and navigate to the login-block page. The page should show `NostrLoginBlock` with detected signer apps listed as tappable cards (or the fallback "no signers installed" + manual key entry view). Capture as `login-block-ios-gallery-preview.png` and add it to the `screenshots` array in `content.ts`.

---

#### 5. `tui-user-core` — screenshots: []

The TUI user-core component (`ProfileWire`) is a pure data type — it has no visual widget to screenshot by itself. The intent is for it to appear alongside a widget that consumes it (e.g., `tui-user-card`). An empty array here is arguably intentional (infrastructure component).

**Does a suitable file exist?** Partial: `tui-user-card-preview.png` shows `ProfileWire` being consumed. There is also `tui-user-avatar-preview.png`.

**Recommended action**: Either leave empty (acceptable for a pure data type — the registry description says "shared Ratatui ProfileWire mirror") OR add `tui-user-card-preview.png` as a representative consumer screenshot with a note that it illustrates the wire type in use. If left empty, the site shows a placeholder which is misleading — this component is "stable", not "coming soon".

---

### Orphan files on disk (not referenced by any registry entry — potential alias pool)

These files exist on disk but are not referenced by any registry entry. They may be residual from earlier naming conventions or may be candidates to fill the dangling references above.

| Orphan filename | Likely source / notes |
|---|---|
| `compose-content-minimal-preview.png` | Compose platform for content-minimal — but content-minimal has NO compose platform entry in the TS registry. Either the registry entry was dropped or never added. |
| `content-core-preview.png` | Generic (no platform suffix) — possibly an early capture; superseded by platform-specific filenames. |
| `content-core-swift-preview.png` | `swift` suffix vs current `ios-gallery` convention — same content, stale naming. Alias for `content-core-ios-gallery-preview.png`. |
| `content-media-grid-preview.png` | Generic — same era as `content-core-preview.png`. |
| `content-media-grid-swift-preview.png` | Stale `swift` suffix. |
| `content-mention-chip-preview.png` | Generic. |
| `content-mention-chip-swift-preview.png` | Stale `swift` suffix. |
| `content-minimal-kotlin-preview.png` | Kotlin preview for content-minimal Compose — no compose entry exists in the registry for content-minimal. Orphaned. |
| `content-minimal-preview.png` | Generic. |
| `content-minimal-swift-preview.png` | Stale `swift` suffix. |
| `content-quote-card-preview.png` | Generic. |
| `content-quote-card-swift-preview.png` | Stale `swift` suffix. |
| `content-view-preview.png` | Generic. |
| `content-view-swift-preview.png` | Stale `swift` suffix. |
| `embed-article-kotlin-preview.png` | Compose/Android article embed — no embed-article Compose entry in the registry. Orphaned. |
| `embed-highlight-kotlin-preview.png` | Same — no embed-highlight Compose entry. |
| `embed-note-kotlin-preview.png` | Same — no embed-note Compose entry. |
| `embed-profile-kotlin-preview.png` | Same — no embed-profile Compose entry. |
| `gallery-bottom.png` | Full-gallery composite bottom slice. Not for a component entry. |
| `gallery-top.png` | Full-gallery composite top slice. Not for a component entry. |
| `relay-list-kotlin-preview.png` | Compose relay-list — no Compose platform for relay-list in the registry. |
| `user-avatar-swift-preview.png` | Stale `swift` suffix. |
| `user-card-swift-preview.png` | Stale `swift` suffix. |
| `user-name-swift-preview.png` | Stale `swift` suffix. |
| `user-nip05-swift-preview.png` | Stale `swift` suffix. |
| `user-npub-swift-preview.png` | Stale `swift` suffix. |

**Immediate candidates for alias use (to fix dangling refs without re-capture):**
- `content-kind-30023-ios-gallery-preview.png` (DANGLING) → alias `embed-article-ios-gallery-preview.png`
- `content-kind-9802-ios-gallery-preview.png` (DANGLING) → alias `embed-highlight-ios-gallery-preview.png`

---

## 3. Per-Entry Verification Criteria

For every screenshot, the reviewer must confirm ALL the following criteria are met — no exceptions, no "probably fine" dismissals.

### content-core (SwiftUI: `content-core-ios-gallery-preview.png`)
- [ ] Screenshot shows a `ContentTreeWire` arena dump: node count + root count visible as monospaced text
- [ ] The identicon preview section shows 4 `NostrIdenticon` circles at sizes 32/40/48/56 with **colored fill** (deterministic color from pablof7z pubkey `fa984bd7...`) — NOT gray/blank circles
- [ ] No pubkey hex or raw npub visible anywhere; this page is intentionally structure-only

### content-core (Compose: `compose-content-core-preview.png`, `content-core-kotlin-preview.png`)
- [ ] Either or both screenshots show the Compose ContentTreeWire or NostrContentRenderer demo
- [ ] No gray placeholder where a rendered element should appear

### content-core (TUI: `tui-content-core-preview.png`)
- [ ] Terminal mockup frame visible (three dots chrome)
- [ ] Shows ContentTreeWire data dump or minimal rendered content in terminal style
- [ ] No empty/blank terminal window

### content-minimal (SwiftUI: `content-minimal-ios-gallery-preview.png`)
- [ ] Prose text "relay note" visible
- [ ] Mention chip shows display name "PABLOF7z" — NOT raw hex (`fa984bd7...`) or raw npub
- [ ] A quoted-note reference (nevent) is rendered as a quote preview, not as a raw `nostr:nevent1...` string

### content-minimal (TUI: `tui-content-minimal-preview.png`)
- [ ] Terminal frame visible
- [ ] Shows inline text with a mention name resolved ("PABLOF7z" or equivalent display name)
- [ ] No raw hex in the mention slot

### content-view (SwiftUI: `content-view-ios-gallery-preview.png`)
- [ ] Full `ContentTreeWire` rendered: prose text + inline mention chip + quote card
- [ ] Mention chip: avatar image present (not blank/gray) + display name "PABLOF7z" — NOT hex
- [ ] Quote card: shows author display name, content preview text, no "loading" / gray placeholder

### content-view (Compose: `compose-content-view-preview.png`, `content-view-kotlin-preview.png`)
- [ ] Same criteria as SwiftUI: resolved display name, no raw identifiers, images loaded

### content-view (TUI: `tui-content-view-preview.png`)
- [ ] Terminal frame visible
- [ ] Block-level content rendered (text paragraph, embedded event or quote card block)
- [ ] No raw nostr URI strings

### content-kind-registry (SwiftUI: `swiftui-content-kind-registry-preview.png`)
- [ ] Shows `NostrKindRegistry` dispatch in action — an embedded event inside `EmbedChromeContainer` (depth-graded accent stripe on left edge)
- [ ] The resolved event is NOT a loading spinner or blank box — it must be the actual rendered kind (article card OR highlight pull-quote OR note preview)
- [ ] Author display name resolved (NOT hex/npub)

### content-kind-registry (SwiftUI: `content-kind-registry-ios-gallery-preview.png`) — MISSING, NEEDS CAPTURE
- [ ] After capture: shows the gallery `ArticleEmbedPage` or a dedicated registry demo page with at least two distinct kind renderers visible side by side or stacked
- [ ] The depth-graded chrome stripe (depth-level accent) is visible and colored (not transparent/invisible)
- [ ] All embed content fully resolved: no "loading" state visible

### content-kind-registry (TUI: shared with `tui-content-view-preview.png`)
- [ ] Same criteria as content-view TUI above

### content-kind-30023 (SwiftUI: `swiftui-content-kind-30023-preview.png`)
- [ ] Article card visible: 16:9 hero image loaded (NOT blank/gray/broken) — Gigi's article "What's left of the internet?" has a hero image
- [ ] Article title "What's left of the internet?" visible
- [ ] Author byline: resolved display name "Gigi" — NOT hex pubkey (`6e468422...`) or raw npub
- [ ] Author avatar image loaded (not identicon/gray, Gigi has a profile picture)

### content-kind-30023 (SwiftUI: `content-kind-30023-ios-gallery-preview.png`) — DANGLING, alias to `embed-article-ios-gallery-preview.png`
- [ ] After fix: same criteria as above. Confirm this is the gallery `ArticleEmbedPage` in the device frame
- [ ] Surrounding prose "hey, check out my article [card] I hope you enjoy it!" — prose text visible ABOVE and BELOW the article card
- [ ] Article card fully resolved (not "loading...") — hero image + title + author byline

### content-kind-30023 (TUI: `tui-embed-article.png`)
- [ ] Terminal frame visible
- [ ] Article title "What's left of the internet?" rendered as a heading
- [ ] Author byline shows "Gigi" — NOT hex
- [ ] Optional summary line present

### content-kind-9802 (SwiftUI: `swiftui-content-kind-9802-preview.png`)
- [ ] Yellow-accented pull-quote box visible — highlighted text inside
- [ ] Highlight body text is actual content (not empty/placeholder)
- [ ] Source footer line visible (URL or event reference)

### content-kind-9802 (SwiftUI: `content-kind-9802-ios-gallery-preview.png`) — DANGLING, alias to `embed-highlight-ios-gallery-preview.png`
- [ ] After fix: yellow accent stripe/box visible
- [ ] Surrounding prose "found this interesting [highlight card]" — prose text visible BEFORE the highlight card
- [ ] Highlight body text resolved (not blank)

### content-kind-9802 (TUI: `tui-embed-highlight.png`)
- [ ] Terminal frame visible
- [ ] Yellow/bold accent block wrapping the highlighted text
- [ ] Source footer line visible

### content-mention-chip (SwiftUI: `content-mention-chip-ios-gallery-preview.png`)
- [ ] Three variants shown: (1) avatar + name, (2) reference fallback (no name loaded), (3) no-avatar variant
- [ ] Variant 1: avatar image loaded (pablof7z photo, not identicon/gray) + display name "PABLOF7z"
- [ ] Variant 2: identicon (colored circles, NOT gray placeholder) + truncated npub format "npub1l2vyh...utajft"
- [ ] Variant 3: name "PABLOF7z" without avatar circle
- [ ] No raw hex in any mention label slot

### content-mention-chip (Compose: both screenshots)
- [ ] Avatar image loaded (Coil SubcomposeAsyncImage resolving pablof7z picture URL)
- [ ] Display name "PABLOF7z" visible — NOT hex
- [ ] Same three variants or equivalent

### content-mention-chip (TUI: `tui-content-mention-chip-preview.png`)
- [ ] Terminal frame
- [ ] Resolved display name or truncated npub (never full hex)

### content-quote-card (SwiftUI: `content-quote-card-ios-gallery-preview.png`)
- [ ] Four variants visible: rich, compact, collapsed, missing
- [ ] Rich variant: author avatar loaded (pablof7z photo), display name "PABLOF7z", note content text (confirmed: "grok cli is INSANELY bad, jesus"), relative timestamp ("Xd ago" / "Xh ago"), optional media thumbnail
- [ ] Compact variant: condensed author + preview, no full content
- [ ] Collapsed variant: "View quote" affordance text visible
- [ ] Missing variant: unresolved reference placeholder — the `nostr:nevent1...` short text, NOT blank
- [ ] NO raw hex or raw unix epoch visible in any variant

### content-quote-card (Compose: both screenshots)
- [ ] Same variant criteria as SwiftUI
- [ ] Resolved display name, no hex

### content-quote-card (TUI: `tui-content-quote-card-preview.png`)
- [ ] Terminal frame
- [ ] Quote card block with author name and content snippet (not raw nevent URI)

### content-media-grid (SwiftUI: `content-media-grid-ios-gallery-preview.png`)
- [ ] At least one image actually loaded — real image pixel content visible (NOT blank/gray/broken-image icon)
- [ ] If multiple images: grid layout visible (2 side-by-side, or 1-large+2-stacked, etc.)
- [ ] No "Waiting for relay-backed media..." text — the media must be resolved from the article

### content-media-grid (Compose: both screenshots)
- [ ] Image loaded (Coil SubcomposeAsyncImage, not blank)
- [ ] Grid layout visible

### content-media-grid (TUI: `tui-content-media-grid-preview.png`)
- [ ] Terminal frame
- [ ] Media URL row(s) or inline image protocol rendered (not empty)

### login-block (SwiftUI) — EMPTY, NEEDS CAPTURE
After capture to `login-block-ios-gallery-preview.png`:
- [ ] `NostrLoginBlock` UI shown — either the signer-detection card list (Amber/Primal/etc.) OR the fallback "no signers detected" + manual key entry view
- [ ] Each signer card has an icon and label (not blank)
- [ ] "Enter key manually" fallback option visible
- [ ] No loading spinner as the final state — detection completes in `.task {}`

---

### Section: User

### user-core (TUI) — EMPTY
If a screenshot is added later (suggested: `tui-user-card-preview.png`):
- [ ] `ProfileWire` fields shown: display_name, npub, nip05 — all kernel-formatted, no Swift/Kotlin truncation

### user-avatar (SwiftUI: `user-avatar-ios-gallery-preview.png`)
- [ ] Large avatar (size 80): pablof7z profile picture LOADED — real photo, not identicon, not gray
- [ ] Size row: three avatars at 32/48/64 — all showing same loaded photo
- [ ] Identicon fallback row: three colored-circle identicons for pablof7z pubkey (deterministic palette color, NOT gray)
- [ ] No broken-image icons anywhere

### user-avatar (Compose: `user-avatar-kotlin-preview.png`)
- [ ] Profile picture loaded (Coil), not identicon
- [ ] Identicon fallback shown separately with correct color

### user-avatar (TUI: `tui-user-avatar-preview.png`)
- [ ] Terminal frame
- [ ] Either inline image (if terminal protocol available) or initials-based identicon visible
- [ ] NOT blank

### user-name (SwiftUI: `user-name-ios-gallery-preview.png`)
- [ ] Display name "PABLOF7z" shown — NOT hex, NOT truncated npub
- [ ] Multiple font size variants shown (default + `.title2`)
- [ ] Avatar + name side-by-side row: avatar photo loaded

### user-name (Compose: `user-name-kotlin-preview.png`)
- [ ] Display name "PABLOF7z" visible
- [ ] Multiple `TextStyle` variants shown

### user-name (TUI: `tui-user-name-preview.png`)
- [ ] Terminal frame
- [ ] Resolved display name "PABLOF7z" (not hex)

### user-nip05 (SwiftUI: `user-nip05-ios-gallery-preview.png`)
- [ ] NIP-05 badge visible with checkmark icon + identifier text
- [ ] Identifier shows DOMAIN ONLY (e.g., "f7z.io" if that is the resolved value) — NOT "_@domain" and NOT the full NIP-05 string including the local-part
- [ ] `init?(profile:)` variant shown (badge present because pablof7z has NIP-05)
- [ ] Direct `init(nip05:)` variant shown

### user-nip05 (Compose: `user-nip05-kotlin-preview.png`)
- [ ] Domain-only NIP-05 (e.g., "f7z.io" if that is the resolved value) — NOT "_@domain" format
- [ ] Checkmark icon rendered (Material icon, not blank)

### user-nip05 (TUI: `tui-user-nip05-preview.png`)
- [ ] Terminal frame
- [ ] Checkmark symbol + domain-only identifier (e.g., "✓ f7z.io" or equivalent) — domain only, no "_@" prefix

### user-npub (SwiftUI: `user-npub-ios-gallery-preview.png`)
- [ ] Truncated npub chip visible: "npub1l2vyh...utajft" (Rust-formatted short form) — NOT full 63-char npub
- [ ] Full npub reference section: the monospaced full npub text visible (for verification context)
- [ ] Chip has a tappable affordance (copy icon or capsule shape)

### user-npub (Compose: `user-npub-kotlin-preview.png`)
- [ ] Truncated npub: "npub1l2vyh...utajft"
- [ ] NOT hex pubkey

### user-npub (TUI: `tui-user-npub-preview.png`)
- [ ] Terminal frame
- [ ] Truncated npub in chip/bracket format

### user-card (SwiftUI: `user-card-ios-gallery-preview.png`)
- [ ] Compact author row: avatar (pablof7z photo, LOADED) + display name "PABLOF7z" + NIP-05 badge (domain-only, not "_@..." format)
- [ ] Larger avatar variant (size 64) row also visible
- [ ] No hex, no "_@", no gray placeholder

### user-card (Compose: `user-card-kotlin-preview.png`)
- [ ] Same: avatar loaded, "PABLOF7z", NIP-05 badge visible (domain-only, no "_@" prefix)

### user-card (TUI: `tui-user-card-preview.png`)
- [ ] Terminal frame
- [ ] Avatar (initials or inline image) + resolved display name + NIP-05 domain

---

### Section: Relay

### relay-list (SwiftUI: `relay-list-ios-gallery-preview.png`)
- [ ] Two relays visible: "purplepag.es" and "relay.primal.net"
- [ ] Connection status dot visible per relay (green dot = connected, yellow = connecting, gray = disconnected) — color is NOT all-gray (at least one must show a non-idle state)
- [ ] Role badge visible per relay: "indexer" for purplepag.es, "both" / "indexer" for relay.primal.net
- [ ] Relay URL shown WITHOUT the "wss://" scheme prefix OR with it — but consistently (no mixed display)
- [ ] No raw hex or pubkey text

### relay-list (TUI: `tui-relay-list-preview.png`)
- [ ] Terminal frame
- [ ] Both relay URLs listed
- [ ] Status indicator character (●/○/etc.) per relay
- [ ] Role badge or label per relay

---

### Section: Embeds & Kinds (website route pages)

### embed-article (SwiftUI: `embed-article-ios-gallery-preview.png`)
- [ ] Surrounding prose ABOVE the card: "hey, check out my article" text visible
- [ ] Article card fully resolved: hero image loaded (NOT blank/gray), title "What's left of the internet?" visible
- [ ] Author byline on card: display name "Gigi" — NOT hex `6e468422...`
- [ ] Gigi's avatar image loaded on the card byline (not identicon)
- [ ] Surrounding prose BELOW the card: "I hope you enjoy it!" text visible
- [ ] The overall layout is prose + card + prose — demonstrating the inline embed pattern

### embed-article (TUI: `tui-embed-article.png`, `tui-embed-article-preview.png`)
- [ ] Terminal frame on both
- [ ] Article heading "What's left of the internet?" rendered
- [ ] Author byline "Gigi" — NOT hex
- [ ] Optional summary paragraph visible
- [ ] `tui-embed-article-preview.png` may show the same content or a slightly different zoom/crop — both must be resolved (no loading state)

### embed-profile (SwiftUI/TUI: `embed-profile-ios-gallery-preview.png`, `tui-embed-profile-preview.png`)
Note: embed-profile is marked `status: "soon"` in the registry. Screenshots reference existing files.
- [ ] iOS: Shows inline mention chip: "met [pablof7z avatar + PABLOF7z] at a nostr conference last week, brilliant mind"
  - Avatar: pablof7z photo loaded (NOT identicon/gray)
  - Mention display name: "PABLOF7z" — NOT hex
  - Surrounding prose text visible around the chip
- [ ] TUI: Terminal frame, inline mention name resolved

### embed-note (SwiftUI/TUI: `embed-note-ios-gallery-preview.png`, `tui-embed-note-preview.png`)
Note: embed-note is marked `status: "soon"`. Screenshots reference existing files.
- [ ] iOS: Shows embedded kind:1 note card: "this is a great point [note card] what do you think?"
  - Note card shows: author display name "PABLOF7z" — NOT hex, note content text (confirmed: "grok cli is INSANELY bad, jesus"), relative timestamp ("Xd ago")
  - Author avatar loaded on note card (not identicon if pablof7z has picture)
  - Surrounding prose text visible around the card
- [ ] TUI: Terminal frame, note content resolved (not raw nevent)

### embed-highlight (SwiftUI: `embed-highlight-ios-gallery-preview.png`)
- [ ] Surrounding prose: "found this interesting" text visible BEFORE the highlight card
- [ ] Yellow-accented pull-quote box visible with highlight body text (actual quoted text, NOT blank)
- [ ] Source footer line (URL or event reference) visible at the bottom of the card
- [ ] No loading state
- Note: the TUI text dump for this embed (`tui-embed-highlight.txt`) shows an empty content pane, meaning the highlight was captured before the relay resolved the kind:9802 event. The iOS screenshot must show it FULLY RESOLVED — if the body text is blank, the capture must be redone after relay resolution.

### embed-highlight (TUI: `tui-embed-highlight.png`, `tui-embed-highlight-preview.png`)
- [ ] Terminal frame on both
- [ ] Yellow/bold highlight block with actual quoted text (NOT empty/blank — the txt dump captured an unresolved state; the PNG should capture a resolved state)
- [ ] Source footer line visible

---

## 4. Naming / Alias Mapping Note

The registry uses two naming conventions and two sets of showcase pages. This table reconciles them:

| Registry naming | Gallery page (iOS Swift) | Gallery page (Kotlin/Android) | Gallery page (TUI Rust) | Screenshot naming convention |
|---|---|---|---|---|
| `content-kind-30023` | `ArticleEmbedPage` | Android gallery article page | `tui` article renderer | `swiftui-content-kind-30023-preview.png`, `embed-article-ios-gallery-preview.png` (alias) |
| `content-kind-9802` | `HighlightEmbedPage` | Android gallery highlight page | `tui` highlight renderer | `swiftui-content-kind-9802-preview.png`, `embed-highlight-ios-gallery-preview.png` (alias) |
| `content-kind-registry` | No dedicated page — exercised via ArticleEmbedPage/HighlightEmbedPage | — | — | `swiftui-content-kind-registry-preview.png` only; `content-kind-registry-ios-gallery-preview.png` MISSING |
| `embed-article` | `ArticleEmbedPage` (same as kind-30023) | — | `tui-embed-article.png` | `embed-article-ios-gallery-preview.png` = alias for `content-kind-30023-ios-gallery-preview.png` |
| `embed-highlight` | `HighlightEmbedPage` (same as kind-9802) | — | `tui-embed-highlight.png` | `embed-highlight-ios-gallery-preview.png` = alias for `content-kind-9802-ios-gallery-preview.png` |
| `embed-profile` | `ProfileEmbedPage` | — | `tui-embed-profile-preview.png` | `embed-profile-ios-gallery-preview.png` |
| `embed-note` | `NoteEmbedPage` | — | `tui-embed-note-preview.png` | `embed-note-ios-gallery-preview.png` |

Key insight: `embed-article` and `content-kind-30023` are the same install path (`swiftui/content-kind-30023`). Their screenshot entries can share the same file. The gallery's `ArticleEmbedPage` is the canonical live demonstration of both registry slugs.

Similarly, `embed-highlight` and `content-kind-9802` point to the same install path (`swiftui/content-kind-9802`).

The embed-* slugs exist as website-only navigation entries (they appear in `SECTIONS` / `COMPONENTS` but NOT in `registry.json`). They give the component showcase richer prose and a dedicated URL, but they install the same files.

---

## 5. Coverage Summary

### Counts per platform

Slot counts: each filename reference in a `screenshots: [...]` array = 1 slot. An EMPTY entry has 0 slots.

| Platform | Components | Screenshot slots (from TS) | OK | DANGLING | Components with EMPTY | NEEDS CAPTURE |
|---|---|---|---|---|---|---|
| SwiftUI | 11 (in registry.json) | 16 slots | 13 OK | 3 DANGLING | 1 (login-block) | 2 new captures needed |
| Compose | 6 (in registry.json) | 11 slots | 11 OK | 0 | 0 | 0 |
| TUI | 10 (in registry.json, incl. user-core) | 10 slots | 10 OK | 0 | 1 (user-core, 0 slots declared) | 0 |

Notes on SwiftUI count: `swiftui-content-kind-registry` has 2 slots (1 OK + 1 DANGLING); `swiftui-content-kind-30023` has 2 slots (1 OK + 1 DANGLING); `swiftui-content-kind-9802` has 2 slots (1 OK + 1 DANGLING); `swiftui-relay-list` has 2 slots both pointing at existing files (one is `tui-relay-list-preview.png` cross-listed); all others have 1 slot each. Total = 16 slots, 13 OK, 3 DANGLING, plus 1 component with 0 slots (login-block, EMPTY).

**Embed & Kinds routes (website-only, not in registry.json):**
| Route | Screenshots | Status |
|---|---|---|
| embed-article | 3 | All OK |
| embed-profile | 2 | All OK |
| embed-note | 2 | All OK |
| embed-highlight | 3 | All OK |

**Total across all registry.json entries + embed routes:**
- Total screenshot slots: 55 (across 41 registry items + embed routes)
- OK: 50
- DANGLING: 3 (all SwiftUI, all in content.ts: content-kind-registry, content-kind-30023, content-kind-9802 second slots)
- EMPTY: 2 (`swiftui-login-block`, `tui-user-core`)
- NEEDS CAPTURE (no suitable alias exists): 2 (`login-block-ios-gallery-preview.png`, `content-kind-registry-ios-gallery-preview.png`)
- FIXABLE BY ALIAS (suitable file exists under different name): 2 (`content-kind-30023-ios-gallery-preview.png` → `embed-article-ios-gallery-preview.png`; `content-kind-9802-ios-gallery-preview.png` → `embed-highlight-ios-gallery-preview.png`)

### Honest verdict

The live site is approximately **91% covered** for the components that have stable screenshots captured. The three dangling references (all SwiftUI, all in `content.ts`) are silent "No screenshot yet" placeholders on the live site right now — users visiting the `swiftui-content-kind-30023` or `swiftui-content-kind-9802` or `swiftui-content-kind-registry` pages see a partial experience: the first screenshot (the `swiftui-*-preview.png`) renders correctly inside its device mockup, but the second slot shows the "No screenshot yet" placeholder.

Two of those three can be fixed in a single TS edit with zero recapture (alias existing files). The third (`content-kind-registry`) and the login-block gap both require a new iOS simulator capture session.

The Compose and TUI platforms are fully covered for every component that has a platform entry — no dangling references, no missing files.

The 26 orphan files on disk are mostly stale (`-swift-preview.png` naming predates the `-ios-gallery-preview.png` convention) and do not affect the live site, but they create confusion and should be cleaned up when the naming convention is finalized.

The quality of the captured screenshots has NOT been verified in this audit (only filename existence was checked). The verification criteria in Section 3 are the oracle for a human reviewer or automated screenshot-diff tool. The highest-risk screenshots for correctness regressions are: any embed that requires relay resolution (article, note, highlight, profile mention), any avatar/media-grid (blank-image failures are silent), and any display-name rendering (hex fallback failures are subtle).

---

## Resolution applied (2026-05-31) — website wiring fixed to verified screenshots only

Every registry `screenshots:` reference now points to a screenshot that was **verified by direct pixel inspection** to show resolved content (display names not hex, real images, "Xd ago" not raw epoch, "f7z.io" not "_@"). The audit checked file *existence*; this pass additionally checked file *content* and caught FIVE screenshots the live site was serving that were actually broken:

| File (was referenced) | What it actually showed | Repointed to (verified) |
|---|---|---|
| `swiftui-content-kind-registry-preview.png` | wrong page (ProfileEmbed) | `embed-article-ios-gallery-preview.png` (article dispatched via NostrKindRegistry) |
| `swiftui-content-kind-30023-preview.png` | byline hex `6e468422…`, not "Gigi" | `embed-article-ios-gallery-preview.png` |
| `swiftui-content-kind-9802-preview.png` | "loading embedded event…" (unresolved) | `embed-highlight-ios-gallery-preview.png` |
| `tui-embed-highlight.png` (content.ts + embeds.ts) | "quote 4fb59c…02393a" (raw hex, unresolved) | `tui-embed-highlight-preview.png` ("Vibe-coding…") |
| `compose-content-{core,view,mention-chip,quote-card,media-grid}-preview.png` | unverified pre-fix Android dups (would show raw epoch on view/quote-card) | dropped; kept the verified post-fix `*-kotlin-preview.png` |

All 48 gallery cells (16 components × iOS/Android/TUI) were re-verified via the all-green verification PDF. Every registry entry now references exactly one verified screenshot per platform.

## Remaining follow-ups (deliberate scope boundary — NOT silently skipped)

1. **`swiftui-login-block` — no screenshot.** `NostrLoginBlock` is a real registry component but the NmpGallery app has **no showcase page** for it, so no screenshot can be captured without first building a gallery login-block page. Left `screenshots: []` (honest "No screenshot yet") rather than fabricate one. To complete: add a login-block showcase page to NmpGallery (sim will show the no-signers fallback + manual key entry), capture `login-block-ios-gallery-preview.png`.
2. **`tui-user-core` — empty by nature.** `ProfileWire` is a pure data type with no visual widget; an empty array is honest. (Could add `tui-user-card-preview.png` as a representative consumer if a placeholder-free card is preferred.)
3. **Android typed article/highlight not published as registry components.** This session built `NostrArticleCard.kt` + `NostrRelativeTime.kt` (Android now renders the typed kind:30023 card), but `content-kind-30023`/`content-kind-9802` only have `swiftui` + `tui` platform entries — no `compose` entry. The verified `embed-article-kotlin-preview.png` therefore has no registry slot. To complete: vendor the new Kotlin components into `web/registry/src/vendor/compose/...` and add `compose` platform entries (full install manifest + screenshot).
4. **26 orphan screenshot files** (stale `*-swift-preview.png` predating the `-ios-gallery-preview.png` convention, etc.) are unreferenced and could be deleted in a separate hygiene pass.
