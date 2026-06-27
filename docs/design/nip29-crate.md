# Design: `nmp-nip29` — NIP-29 Relay-Based Groups as a Protocol Crate (M11.5)

> **Status:** Implemented as generic protocol infrastructure; updated 2026-06-26 to reflect the current projection/action/read-interest surface. **Date:** 2026-05-18.
> **Companion docs:** `docs/design/subscription-compilation.md` §§ 4, 7 (the M2 planner this crate hooks into); `docs/product-spec/doctrine.md` §D0 (current extension seams); ADR-0009 (the kernel-boundary doctrine the crate must respect).
> **Scope:** Define the public surface, internal architecture, and routing
> contract of `nmp-nip29` — the NMP-idiomatic protocol crate for NIP-29
> relay-based groups. Current code expresses this with typed action
> registration, snapshot projections, hydrating observed read interests, and
> host-pinned filter builders.

This document is split into focused sub-files to stay well under the 300 LOC ceiling per file.

- [Routing contract — the host-relay-pin and how it lands in the M2 compiler / publish planner](nip29/routing.md)
- [Kinds catalog — every NIP-29 event kind, mapped to NMP ingest + module ownership](nip29/kinds.md)
- [Moderation + previous-tag chain — relay-signed metadata, admin-signed actions, ingest-time validation](nip29/moderation.md)

## 1. Why this needs to be its own crate

NIP-29 is *structurally different* from every other NIP the framework has touched in M0–M11:

1. **Groups are identified by a `(host_relay_url, group_id)` pair**, not by an event-id or addressable coordinate. Two relays running with the same `group_id` are two different groups. Highlighter's existing core dodges this by hardcoding `HIGHLIGHTER_RELAY` (`relays.rs:24`); NMP cannot.
2. **Group metadata events (39000–39003) are signed by the relay**, not by any human user. They appear in the user's stream out of nowhere and must be trusted because the relay produced them, not because a follow did.
3. **Every group event has a forced routing target**: the host relay. NIP-65 mailbox routing for the author is *irrelevant* for these events — they only exist on that one relay. This is the routing-contract inversion that justifies the whole crate existing.
4. **The `previous`-tag chain** is a relay-enforced anti-forgery mechanism with no analog in any other Nostr NIP. *Outbound* publishes must attach `previous` references (per `moderation.md` §2); *ingest* preserves the tags but does not re-validate (the relay already did, and re-validating client-side would risk dropping valid events during cold-cache / historical backfill).
5. **The "group" is a security boundary**, not just a noun. A private group's 39000 may be hidden from non-members; a closed group rejects join requests. The crate's projections must respect membership state when projecting.

Treating NIP-29 as "just another kind range" would force NMP's kernel actor or M2 compiler to grow group-aware special cases, violating ADR-0009 doctrine D0 ("the kernel never grows app nouns"). The crate is the boundary where the special cases live; the kernel sees a generic `RelayPinned` and `RelayPinnedPublish` it already knows how to handle (see `routing.md`).

## 2. Crate placement in the workspace

```
crates/
├── nmp-core/                  # kernel substrate — D0: knows nothing of groups
├── nmp-codegen/
├── nmp-nip01/                 # profiles (kind:0) — exists post-M1
├── nmp-nip02/                 # follows (kind:3) — exists post-M2
├── nmp-nip17-nse/             # DMs — deferred to post-v1
├── nmp-nip22/                 # comments (kind:1111)
├── nmp-nip23/                 # long-form (kind:30023)
├── nmp-nip25/                 # reactions (kind:7)
├── nmp-nip29/                 # ← THIS CRATE — relay-based groups
├── nmp-nip51/                 # lists
├── nmp-nip65/                 # mailboxes (kind:10002) — exists post-M2
├── nmp-nip78/                 # app data (kind:30078)
├── nmp-nip84/                 # highlights (kind:9802)
├── nmp-blossom/               # media uploads
└── nmp-testing/
```

Apps consume `nmp-nip29` through the app's Rust composition layer. The protocol
crate provides reusable NIP-29 building blocks; app crates decide which read
views to open and which actions to expose.

## 3. Current public surface produced by `nmp-nip29`

`nmp-nip29` currently ships four categories of reusable surface:

| Surface | Current implementation | Notes |
|---|---|---|
| Typed group identity | `GroupId { host_relay_url, local_id }` plus host-pinned filter builders such as `GroupId::chat_filter_json` and `group_metadata_filter_json` | Every read/write path carries the host relay explicitly; a bare `h` value is never enough to identify a group. |
| Read projections | `GroupChatProjection`, `DiscoveredGroupsProjection`, `JoinedGroupsProjection`, and `GroupDefaultsProjection` | Per-open read views are hydrated through observed interests from `nmp-ffi::group_feed`; the default snapshot is registered directly by `wire_group_defaults*`. |
| Actions | `register_actions` installs the supported NIP-29 write actions with the app action registrar | Writes remain protocol-owned and host-pinned; the app shell does not derive relay routing from UI state. |
| Small protocol caches/helpers | `RecentGroupEvents`, `JoinedHostsCache`, input-scope recognizers, and previous-tag helpers | These are protocol-internal helpers; durable app read state is exposed through projections and snapshot sidecars. |

The app-side composition in `nmp-ffi::group_feed` registers a projection muted,
opens a host-pinned observed interest, replays matching cached events, then
activates the projection. That is the current read-model contract for
late-opened NIP-29 views; a plain active event observer is not a valid
hydrating view path.

When an app-specific Rust projection needs to compose over group timeline,
discovered groups, or joined groups, it must use the `nmp-ffi::group_feed`
reader-returning open methods (`open_group_timeline_with_reader`,
`open_group_discovery_with_reader`, `open_joined_groups_with_reader`). Those
methods return the same projection instance that feeds the canonical typed
sidecar, preserving one producer for each `nmp.nip29.*` projection key.

### 3.1 What `nmp-nip29` does **not** ship

- **No `CapabilityModule`.** The crate uses existing capabilities (signer, http, blossom for picture uploads) but doesn't add any.
- **No signer/account ownership.** Groups do not change the user's identity
  model; signer/account state stays in the session/signers layer.
- **No app-owned persistence schema in `nmp-core`.** NIP-29 read models are
  protocol-owned Rust state surfaced through projections.

## 4. The load-bearing constraint: host-relay-pin

The single most important property of NIP-29 — and the property every other design decision in this crate falls out of — is that **group operations bypass NIP-65 routing and pin to the group's host relay**.

This contradicts the M2 outbox planner's default behavior, which routes:

- subscriptions with `authors` → those authors' write relays
- subscriptions with `#p` → those pubkeys' read relays
- publishes → the author's write relays + recipient inbox relays for tag-targeted recipients

NIP-29 needs a **third routing lane**: the `h`-tag lane. Any filter with `#h: [group_id]` routes to the host relay; any publish of an event with an `h` tag routes to the host relay. The author's NIP-65 mailboxes don't enter into it.

Full design of how this lands in the compiler and publisher is in [`nip29/routing.md`](nip29/routing.md). The summary is: `nmp-nip29` declares its interests as a typed `RelayPinned` (carries `host_relay_url` explicitly), and the M2 compiler short-circuits its three-lane logic when it sees that variant.

## 5. The "group identity" type

A `GroupId` in `nmp-nip29` is **not** a bare string. It is:

```rust
pub struct GroupId {
    pub host_relay_url: RelayUrl,   // normalized per NIP-65 url-canonicalization
    pub local_id: String,           // matches NIP-29 charset [a-z0-9-_]+
}
```

This is the breaking change versus Highlighter's older core, which treated `group_id` as a `String` because there was only ever one host. Every current projection, action input, and filter builder that references a group uses `GroupId` or equivalent `(host_relay_url, local_id)` data. The kernel still sees generic interests, projections, and publish actions — no kernel group noun is needed.

For UI surfaces that need a flat string (URLs, deep links, share cards), `nmp-nip29::GroupId` provides `to_uri()` / `from_uri()` round-tripping into the NIP-29 spec format `<host>'<local-id>` (e.g. `groups.nostr.com'abcdef`).

## 6. Cross-crate joins (resolved at the app layer, not inside `nmp-nip29`)

The user surfaces a product consumes may need joins against other crates. Per the NMP boundary rule, `nmp-nip29` does not import sibling `nmp-nip*` crates for product presentation joins. Those joins live in the app's Rust composition layer, where they compose NIP-29 projections with profile, comment, highlight, or artifact projections.

| Composed view (app Rust layer) | Composes | Mechanism |
|---|---|---|
| `HydratedGroupChat` | `nmp-nip29::GroupChat` + `nmp-nip01::Profile` for each author | composite-key dependency tracking at the substrate level (ADR-0001) — `highlighter-core::HydratedGroupChat::dependencies()` enumerates both; the kernel reverse-index handles the join with no protocol-crate awareness |
| `DiscussionsWithReplyCounts` | `nmp-nip29::GroupDiscussions` + `nmp-nip22::Comment { e: <discussion_id> }` per discussion root | Discussion replies in Highlighter today are *non-h-tagged* NIP-22 comments (verified per `kinds.md` §2.1 notes), so they live in `nmp-nip22` and route per the replier's NIP-65 write relays. The count + latest-reply join happens in `highlighter-core`'s `project()`. (If/when Highlighter changes its composer to attach `h`, the dependency shifts to `nmp-nip29::GroupComment` with no other code change — that's the point of the substrate's generic composite-key joins.) |
| `GroupArtifactLanes` | `nmp-nip29::GroupArtifacts` (which already surfaces both kind:11 artifact shares + kind:16 reposts per §3.2) + `nmp-nip84::Highlight` deref'd from each share/repost's referenced event (`e` tag → `nmp-nip84::Highlight` for highlight reposts; `r`/`i`/`a` catalog tag → external artifact lookup for native artifact shares) | the deref chain happens in `highlighter-core`'s projection; `GroupArtifacts` is what subscribes (and therefore what makes the lanes update on new shares) |

Why this works: the kernel's projection and interest machinery is generic. It does not care which protocol crate owns the projection data; the app Rust layer composes typed snapshots and opens whatever explicit interests that composed view needs.

`nmp-nip29` ships its own non-hydrated views (`GroupChat`, `GroupDiscussions`, `GroupHome`, etc.) that are useful on their own (debugging UIs, tests, headless clients) without any cross-crate joins. The hydrated variants are app-level conveniences.

For kind:16 (generic repost): the *h-tagged* repost is owned by `nmp-nip29::GroupRepost` in M11.5, because the routing is host-pin and no separate `nmp-nip18` crate exists yet (per §3.1). A future `nmp-nip18` extraction would lift only the *non-h* repost case out; the h-tagged variant stays in `nmp-nip29` either way because routing is the discriminator. `highlighter-core` never grows kind:16 ingest in any state — the consistency contract is firm.

## 7. What's deferred vs in-scope

**In current v1 scope:**

- Typed `GroupId` and host-pinned filter builders for group chat and group metadata acquisition.
- Hydrating read projections for group chat, discovered groups, joined groups, and app/operator group defaults.
- Action registration for the supported NIP-29 write actions.
- Metadata/moderation parsing rules described in the sub-files, with relay-signed metadata treated as relay-owned protocol state.

**Deferred to a follow-up milestone or to relay-side implementation:**

- **Group migration** (NIP-29 supports groups moving between relays; UI for "this group moved, follow it to its new host?" is post-M11.5).
- **Group forking** (same `local_id` on two relays = two groups; UI to disambiguate post-M11.5).
- **Manual key-rotation UX** for the metadata-signer trust model (the typed `MetadataSignerChanged` rejection lands in M11.5 per `moderation.md` §4.3; an interactive "trust the new key?" prompt is post-M11.5 polish). Note: metadata-signer pinning *itself* (policy A + B) IS in M11.5 scope and is NOT deferred — see §8 question 2.
- **`hidden` group support** (a metadata flag that hides the 39000 from non-members; we recognize the flag in projection but don't ship a UI for hidden-group invites in M11.5).

## 8. Open questions for follow-up ADRs

1. **Where does the host-relay-pin routing rule live in the planner?** Two viable shapes: (a) `nmp-nip29` returns a typed `RelayPinned` that the compiler's outer dispatch handles, vs (b) the compiler grows a generic "honor pin-hints from any crate" mechanism and `nmp-nip29` participates via a trait. (b) is cleaner long-term (other relay-pinned NIPs may emerge) but (a) ships M11.5 faster. ADR needed.
2. **Trust model for relay-signed metadata.** Resolved: M11.5 ships policy A (NIP-11 strict) when NIP-11 declares a relay pubkey, otherwise B (TOFU per `(host_relay_url, group_id)`). Policy C (accept-any-from-host) is explicitly rejected — codex review surfaced a P1 spoofing vector where any host relay also accepting parameterized writes lets a malicious user forge a 39001/39002 admitting themselves as admin. See `moderation.md` §4. The remaining ADR-level question is rotation UX (silent accept on first warning vs explicit prompt); deferred to post-M11.5.
3. **`JoinedGroups` aggregation across multiple host relays.** Resolved by ADR-0060. `nmp-nip29` owns the joined-groups projection, fans out through host-pinned 39001/39002 interests, and derives joined/admin status only from relay-signed snapshots.
4. **Membership-as-security-boundary in projections.** Resolved: M11.5 **gates private-group projections** explicitly per `moderation.md` §6. The gate empties `GroupChat`/`GroupDiscussions`/`GroupMembers`/`GroupHome` for any group whose 39000 carries the `private` marker AND whose latest 39002 does not contain the current user's pubkey. Public groups are never gated (the room preview surface in `feature-inventory.md` §1.1 needs this). This is mandatory, not best-effort — without the gate, cached private-group chat would leak after the user is removed from the group.
5. **Invite-code redemption UX vs JoinRequest.** The 9021 with a `code` tag is the redemption path. The current Highlighter onboarding lets the user paste a code before any signer is installed. Does the redemption action defer until a signer exists, or do we mint a fresh local key and redeem immediately? Cross-cuts M6 (signer flows). ADR-level question.
6. **Tombstoning on kind:9008 (delete-group).** The relay can delete a group entirely. What should projections and any durable app-side state keyed under `(host_relay_url, group_id)` do? Bias: remove those records (the relay no longer serves them; provenance dies with the group), surface a one-shot "group deleted" notification through the diagnostics lane.

The three sub-files (`nip29/routing.md`, `nip29/kinds.md`, `nip29/moderation.md`) work through these in detail.
