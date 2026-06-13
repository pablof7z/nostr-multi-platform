---
type: research-record
date: 2026-06-13
session: 027459be-7102-4e1a-b6d4-02e8e7863642
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/027459be-7102-4e1a-b6d4-02e8e7863642.jsonl
source_lines: 7340-7471
agent_attribution: Audit new #1294 Android claim components
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Audit of Android Compose claim-host components across 5 dimensions (claim lifecycle, D0 thin-shell, dedup, image loading, LazyColumn recycling) with HIGH/MEDIUM/LOW/CLEAN verdicts; found HIGH claim-churn loop bug"
captured_at: 2026-06-13T19:26:40Z
---

Audit of Android Compose claim-host components across 5 dimensions (claim lifecycle, D0 thin-shell, dedup, image loading, LazyColumn recycling) with HIGH/MEDIUM/LOW/CLEAN verdicts; found HIGH claim-churn loop bug

---

User: [Agent task result: Agent "Audit new #1294 Android claim components" completed]
Now I have a complete picture. Let me compile the findings.

---

## Audit Findings: Android Compose Claim-Host Components (commit de618805d / PR #1294)

### (1) CLAIM LIFECYCLE

**HIGH — Claim/release churn on every snapshot tick (same bug class as chirp-web PR 4d1888f9a)**

`NostrAvatar.kt:47` and `NostrProfileName.kt:80` key their `DisposableEffect` on `(pubkey, profileHost, resolvedConsumerId)`. The `profileHost` slot is the `KernelProfileHost` instance from `LocalNostrProfileHost.current`.

`KernelProfileHost.kt:80` creates the host with `remember(model, profiles)`, where `profiles` is `resolvedProfiles` — a new `Map<String, ProfileCard>` object deserialized on every snapshot frame. Because `remember` computes equality on map object identity (not structural equality), every snapshot tick produces a new `KernelProfileHost` instance. That new instance propagates through `LocalNostrProfileHost` to every `NostrAvatar` and `NostrProfileName` descendant. Their `DisposableEffect(pubkey, profileHost, ...)` sees the host key change, fires `onDispose` (release), then re-runs the effect (re-claim). The re-claim triggers a kernel response (profile data), which emits a new snapshot, which produces a new map, which creates a new host — the classic claim/release → snapshot → new host → DisposableEffect re-fire loop documented in the chirp-web fix (commit 4d1888f9a: "an infinite churn loop that prevented Bob's display name from ever stabilising").

Files/lines:
- `android/app/src/main/java/org/nmp/android/components/NostrAvatar.kt:47` — `DisposableEffect(pubkey, profileHost, resolvedConsumerId)`
- `android/app/src/main/java/org/nmp/android/components/NostrProfileName.kt:80` — same pattern
- `android/app/src/main/java/org/nmp/android/ui/KernelProfileHost.kt:80` — `remember(model, profiles)` with a new map on every tick

Fix: either remove `profileHost` from the `DisposableEffect` key (it should not be a key — claim/release are forwarded to a stable `KernelModel`, not owned by the host instance), or make `rememberKernelProfileHost` stable by keying on `model` only and using `rememberUpdatedState(profiles)` inside `profileForPubkey`.

**LOW — UUID-based consumerId is stable within a composition slot but wasteful when `consumerId` param is null**

`NostrAvatar.kt:42`: `remember(pubkey, consumerId) { consumerId ?: "nostr-avatar.${UUID.randomUUID()}" }`. When `consumerId` is `null`, a UUID is minted once per composition slot and is correctly stable across recompositions. The UUID is NOT re-rolled on pubkey changes (the `pubkey` key in `remember` ensures a new UUID is generated when pubkey changes, which is correct — the old claim is released and a new one is registered). This is acceptable, but it means two `NostrAvatar` components for the same pubkey (e.g., two visible rows with the same author) register two separate consumer IDs, which is intentional and correct for the refcount model (Rust deduplicates at the `(scope, key)` level per `registry.rs`). No bug here, just noting the pattern.

**CLEAN — DisposableEffect releases on dispose and re-keys on pubkey change**

When `pubkey` changes (LazyColumn rebinds a row to a different author), `remember(pubkey, consumerId)` produces a new `resolvedConsumerId`, which causes `DisposableEffect` to re-run — releasing the old claim and registering a new one. This is correct. There is no claim leak from pubkey rebinding.

**CLEAN — ProfileScreen own claim is correct**

`ProfileScreen.kt:54-60`: `val profileConsumerId = "profile_screen-$pubkey"` is a stable string, keyed in `DisposableEffect(pubkey)`. Release fires in `onDispose`. No leak.

**MEDIUM — DmConversationListScreen double-collects `model.state`**

`DmScreen.kt:63` (`DmScreen`) collects `model.state` to derive `dmInbox`, and `DmConversationListScreen.kt:99` collects it again independently to derive `resolvedProfiles`. This is redundant subscription on the same `StateFlow` but functionally harmless — Kotlin `StateFlow` is multicast with no side effects per collector. However, it means `DmConversationListScreen` is passed `dmInbox` from the parent's snapshot tick but derives `resolvedProfiles` from its own tick, which can be a different frame. This is a minor consistency hazard (profiles and conversations could reflect different snapshot generations), not a crash/leak.

File: `android/app/src/main/java/org/nmp/android/ui/DmScreen.kt:63` and `DmScreen.kt:99`

**CLEAN — DmConversationView thread claim**

`DmScreen.kt:232-235`: `DisposableEffect(peerPubkey)` claims `"dm-thread"` consumer, releases on dispose. Stable consumer ID, correct teardown. The `NostrProfileHost` is not provided into `DmConversationView` (no `CompositionLocalProvider` wraps it), so `NostrAvatar` would receive `null` from `LocalNostrProfileHost` and skip the avatar. There is no avatar render in `DmConversationView`, so this is not a bug — the thread view correctly shows only text messages.

---

### (2) THIN-SHELL (D0) — ProfileWire / ProfileCard

**CLEAN — No kind:0 JSON parsing in Kotlin**

`ProfileWire.kt` is a pure data-transfer type: `@Serializable data class ProfileWire(...)` with no protocol logic. It receives pre-decoded fields from `KernelProfileHost.profileForPubkey`. No raw kind:0 event content is ever parsed in Kotlin.

`KernelProfileHost.kt:44-57`: `profileForPubkey` reads directly from `ProfileCard` (the Rust projection) and maps field-by-field into `ProfileWire`. There is one computation here that is a potential D0 flag: `shortHex(pubkey)` and `shortHex(npub)` at `KernelProfileHost.kt:49` produce Kotlin-side abbreviations. These are presentation-layer text transformations (truncation for display), not protocol parsing. This is consistent with the documented iOS behaviour (`account.npub.shortHex`, PR #1064) and is not a D0 violation.

`TypedProfilesDecoder.kt` decodes FlatBuffers projection bytes — no raw Nostr event parsing, no kind:0 JSON decoding in Kotlin. The Rust kernel pre-parses kind:0 content and ships `ProfileCard` struct fields directly.

---

### (3) KernelProfileHost / claim de-duplication

**CLEAN at the Rust level — N avatars for the same pubkey = 1 REQ**

`crates/nmp-core/src/subs/registry.rs:1-10`: the `InterestRegistry` keeps "one live `LogicalInterest` per `(scope, key)` and refcounts owners." Test `kernel_reducer/tests_snapshot_claims.rs:115` (`claim_profile_refcount_dedup_does_not_double_fetch`) explicitly asserts two different `consumer_id`s for the same pubkey do not double-fetch. Android correctly passes through unique consumer IDs per component instance; the Rust layer deduplicates the REQ.

**MEDIUM — No eviction path visible in the Android host map**

`KernelProfileHost.kt:36`: the `profiles` map is the snapshot's `resolvedProfiles` — it is a snapshot of what the kernel knows. The kernel's interest registry owns eviction through `release_profile` refcount drain. On the Android side there is no local cache; each new `KernelProfileHost` holds a reference to the current snapshot map only. Because `rememberKernelProfileHost` uses `remember(model, profiles)`, a new instance is created on every snapshot (see item 1). This means the host instance does not accumulate entries — the map is replaced wholesale. No growing host map; no eviction needed on the Kotlin side. This is clean given the current architecture (Kotlin is stateless, Rust owns the cache), but is coupled to the churn bug above.

---

### (4) IMAGE LOADING

**CLEAN — Coil `SubcomposeAsyncImage` with error/loading placeholders**

`NostrAvatar.kt:65-71`:
```kotlin
SubcomposeAsyncImage(
    model = resolvedAvatarUrl,
    contentDescription = null,
    modifier = baseModifier,
    error = { NostrIdenticonBox(pubkey = pubkey, size = size) },
    loading = { NostrIdenticonBox(pubkey = pubkey, size = size) },
)
```
Both `error` and `loading` slots are populated with the identicon fallback. Coil 2.6.0 handles all network I/O on its own dispatcher; there is no main-thread network access. Coil's default `ImageLoader` uses an LRU memory cache bounded by 25% of available heap and a disk cache bounded by 250 MB — appropriate for a mobile app.

**CLEAN — No D1 render gate**

There is no `if (resolvedAvatarUrl == null) return` that would blank the avatar while the kind:0 is in-flight. The identicon is shown during loading, then replaced by the real picture when Coil fetches it. This matches the D1 best-effort render requirement.

---

### (5) CONSUMER CORRECTNESS — LazyColumn recycling / claim scoping

**CLEAN — NoteRow claims are scoped to row lifetime**

`TimelineScreen.kt:375-379`:
```kotlin
NostrAvatar(
    pubkey = authorPubkey,
    size = 36.dp,
    consumerId = "note-author-$eventId",
)
```
The `consumerId` is `"note-author-$eventId"` — stable per event, not per pubkey. When LazyColumn recycles a row slot to a different event (different `eventId`), `remember(pubkey, consumerId)` generates a new internal consumer ID, causing `DisposableEffect` to release the old claim and register a new one. Scrolled-away rows are disposed by Compose (LazyColumn's item disposal), which fires `onDispose` → `releaseProfile`. Working set growth on scroll is prevented.

**CLEAN — DmConversationRow**

`DmScreen.kt:176-179`: `consumerId = "dm-peer-${conversation.peerPubkey}"` — stable per peer, correctly scoped. When the row scrolls out, the `NostrAvatar`'s `onDispose` releases the claim.

**NOTE — ThreadScreen does not provide `LocalNostrProfileHost` for `NoteRow` embedded in `NostrRichText`**

`ThreadScreen.kt` provides `LocalNostrProfileHost` at its column root (line confirmed above). However, `NoteRow` uses `LocalNostrProfileHost.current` only through `NostrAvatar`, which correctly falls back to `null` (the `profileHost?.claimProfile` guard). The thread screen is also not yet wired to a `LocalProfileClaimer` — `RememberProfileClaim` calls are no-ops in `ThreadScreen` because `LocalProfileClaimer` is not provided there (it is provided in `TimelineScreen` and `DmConversationListScreen`). This means author display names in `ThreadScreen` fall back to `resolvedProfiles` from the screen's local map, not from self-claiming components. This is a functional gap (no on-demand fetch for thread authors), but not a leak or crash.

File: `android/app/src/main/java/org/nmp/android/ui/ThreadScreen.kt` — `LocalProfileClaimer` not provided.

---

## Summary Table

| # | Finding | File:Line | Bug Class | Severity |
|---|---------|-----------|-----------|----------|
| 1 | `DisposableEffect` keyed on `profileHost` which changes identity on every snapshot tick → claim/release churn loop | `NostrAvatar.kt:47`, `NostrProfileName.kt:80`, `KernelProfileHost.kt:80` | Claim churn / same bug class as chirp-web 4d1888f9a | HIGH |
| 2 | `DmConversationListScreen` double-collects `model.state` independently of parent; profiles and conversations can reflect different snapshot generations | `DmScreen.kt:63,99` | Minor consistency hazard | MEDIUM |
| 3 | `ThreadScreen` does not provide `LocalProfileClaimer`; thread author names never trigger on-demand kind:0 fetch | `ThreadScreen.kt` | Functional gap (no crash/leak) | LOW |
| D0 | ProfileWire is a typed projection; no kind:0 JSON parsed in Kotlin | — | CLEAN | — |
| Dedup | Rust `InterestRegistry` deduplicates N consumers for same pubkey to 1 REQ; tested | — | CLEAN | — |
| Image | Coil `SubcomposeAsyncImage` with error/loading placeholders; no main-thread I/O | `NostrAvatar.kt:65-71` | CLEAN | — |
| LazyColumn | Claims scoped to row lifetime via `onDispose`; pubkey rebind releases old claim | `TimelineScreen.kt:375-379` | CLEAN | — |
| Lifecycle | `ProfileScreen` and `NostrAvatar`/`NostrProfileName` each release on dispose; pubkey-keyed | `ProfileScreen.kt:55-60`, components | CLEAN | — |

**The most important finding is item 1**: it is the exact same churn loop that was fixed in chirp-web (commit 4d1888f9a). The fix there was to use stable sub-memos that do not change on claim/release responses. The Android fix is to remove `profileHost` from the `DisposableEffect` key in `NostrAvatar` and `NostrProfileName` — the host object identity must not be a lifecycle key because it changes on every snapshot. The claim/release themselves forward to the stable `KernelModel` bridge, not to the host instance, so dropping the host from the key is safe and correct.
