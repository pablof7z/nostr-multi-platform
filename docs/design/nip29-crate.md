# Design: `nmp-nip29` — NIP-29 Relay-Based Groups as a Protocol Crate (M11.5)

> **Status:** Draft (M11.5 design + impl-prep). **Date:** 2026-05-18.
> **Companion docs:** `docs/plan.md` §M11 (the template this follows); `docs/plan/m11.5-highlighter.md` (the milestone using this crate); `docs/research/highlighter/app-survey.md` + `feature-inventory.md` (the empirical inputs); `docs/design/subscription-compilation.md` §§ 4, 7 (the M2 planner this crate hooks into); `docs/design/kernel-substrate.md` §§ 3–4 (`ViewModule`, `ActionModule`); ADR-0009 (the kernel-boundary doctrine the crate must respect).
> **Scope:** Define the public surface, internal architecture, and routing contract of `nmp-nip29` — the NMP-idiomatic protocol crate for NIP-29 relay-based groups. **This is a design doc; no implementation lands in this PR.**

This document is split into focused sub-files to stay well under the 300 LOC ceiling per file.

- [Routing contract — the host-relay-pin and how it lands in the M2 compiler / publish planner](nip29/routing.md)
- [Kinds catalog — every NIP-29 event kind, mapped to NMP ingest + module ownership](nip29/kinds.md)
- [Moderation + previous-tag chain — relay-signed metadata, admin-signed actions, ingest-time validation](nip29/moderation.md)

## 1. Why this needs to be its own crate

NIP-29 is *structurally different* from every other NIP the framework has touched in M0–M11:

1. **Groups are identified by a `(host_relay_url, group_id)` pair**, not by an event-id or addressable coordinate. Two relays running with the same `group_id` are two different groups. Highlighter's existing core dodges this by hardcoding `HIGHLIGHTER_RELAY` (`relays.rs:24`); NMP cannot.
2. **Group metadata events (39000–39003) are signed by the relay**, not by any human user. They appear in the user's stream out of nowhere and must be trusted because the relay produced them, not because a follow did.
3. **Every group event has a forced routing target**: the host relay. NIP-65 mailbox routing for the author is *irrelevant* for these events — they only exist on that one relay. This is the routing-contract inversion that justifies the whole crate existing.
4. **The `previous`-tag chain** is a relay-enforced anti-forgery mechanism with no analog in any other Nostr NIP. Ingest must validate.
5. **The "group" is a security boundary**, not just a noun. A private group's 39000 may be hidden from non-members; a closed group rejects join requests. The crate's view modules must respect membership state when projecting.

Treating NIP-29 as "just another kind range" would force NMP's kernel actor or M2 compiler to grow group-aware special cases, violating ADR-0009 doctrine D0 ("the kernel never grows app nouns"). The crate is the boundary where the special cases live; the kernel sees a generic `RelayPinnedInterest` and `RelayPinnedPublish` it already knows how to handle (see `routing.md`).

## 2. Crate placement in the workspace

```
crates/
├── nmp-core/                  # kernel substrate — D0: knows nothing of groups
├── nmp-codegen/
├── nmp-nip01/                 # profiles (kind:0) — exists post-M1
├── nmp-nip02/                 # follows (kind:3) — exists post-M2
├── nmp-nip17-nse/             # DMs — DEFERRED to post-v1 per scope-adjustments
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

Apps consume `nmp-nip29` by adding it to their `nmp.toml` enabled-modules list, which causes `nmp-codegen` to register its DomainModules / ViewModules / ActionModules into the generated per-app crate (per ADR-0010).

## 3. Trait family families produced by `nmp-nip29`

Per the advisor checkpoint and verified against the Highlighter `app/core/src/{groups,chat,discussions}.rs` public surfaces, the crate produces **three** of the five substrate trait families (per `crates/nmp-core/src/substrate/mod.rs`):

### 3.1 `DomainModule` impls (9)

These are the persistent record shapes the crate owns. Each is the truth source for its kind range; views project off them.

| `DomainModule` | Owns kinds | Composite keys | Notes |
|---|---|---|---|
| `Group` | 39000 (metadata) | `(host_relay_url, group_id)` | The metadata snapshot for a group; replaceable on every 9002 edit → 39000 republish. |
| `GroupAdmins` | 39001 | `(host_relay_url, group_id)` | The admin set; replaceable on every 9000 with `role` tag → 39001 republish. |
| `GroupMembers` | 39002 | `(host_relay_url, group_id)` | The member set; replaceable on every 9000/9001 → 39002 republish. |
| `GroupRoles` | 39003 | `(host_relay_url, group_id)` | The relay's declared role list (optional; not all relays publish 39003). |
| `GroupChatMessage` | 9 (when `h` present) | `(host_relay_url, group_id, event_id)` | Flat chat message. The `h` tag is the in-group key. |
| `GroupDiscussion` | 11 (when `h` present and `["t","discussion"]` marker) | `(host_relay_url, group_id, event_id)` | Threaded discussion root. Replies are NIP-22 kind:1111 owned by `nmp-nip22` but cross-referenced. |
| `GroupArtifact` | 11 (when `h` present and *no* `t=discussion` marker — Highlighter's "share an article/podcast/book into a room" event per `artifacts.rs::publish`) | `(host_relay_url, group_id, event_id)` | The "Suggest an artifact" / Room Library source events. Distinct from `GroupDiscussion` by the absence of `t=discussion`; the artifact reference is in catalog tags (`r`/`i`/`a` per Highlighter's `web/src/lib/ndk/artifacts.ts` convention). Tagged as Highlighter convention in `kinds.md` §2.1 — `nmp-nip29` ships ingest because the wire-level discriminator (`t=discussion` present/absent) is structural enough to model at the protocol-crate level. |
| `GroupRepost` | 16 (when `h` present) | `(host_relay_url, group_id, event_id)` | NIP-18 generic repost scoped into the group. The "share an existing highlight into a community" path (`highlights.rs::share_to_community`, `highlights.rs::publish_and_share`). Lives in `nmp-nip29` rather than a separate `nmp-nip18` crate because (a) `nmp-nip18` doesn't exist yet, (b) the routing concern is the `h`-tag, not the kind, (c) the surface is small (one DomainModule + one Action). A future `nmp-nip18` extraction would lift the non-`h` repost case out. |
| `GroupModerationEvent` | 9000–9009, 9021, 9022 | `(host_relay_url, group_id, event_id)` | Audit trail of admin-side actions. Not directly user-rendered; used by the moderation view + as evidence in dispute / forensic UI. Schema in `moderation.md` §5. |

### 3.2 `ViewModule` impls (7)

These are the projections the UI consumes. Each declares `LogicalInterest`s the M2 compiler turns into wire-level REQs, with routing pinned to the host relay per `routing.md`.

| `ViewModule` | Composite dependency keys | Surfaces |
|---|---|---|
| `JoinedGroups` | `(current_pubkey, host_relay_url)` | The "communities I'm in" list — derived from any 39001/39002 containing `current_pubkey` across all host relays the user has joined groups on. The hardest view to design — see `routing.md` §4 on cross-relay aggregation. |
| `GroupHome` | `(host_relay_url, group_id)` | Single-group landing page: metadata + admin/member counts + recent chat + recent discussions + member preview. |
| `GroupChat` | `(host_relay_url, group_id)` | Ordered ascending kind:9 messages in one group. |
| `GroupDiscussions` | `(host_relay_url, group_id)` | List of kind:11 *discussion-marked* roots in one group, ordered by latest-reply timestamp. |
| `GroupArtifacts` | `(host_relay_url, group_id)` | List of `GroupArtifact` records (kind:11 without `t=discussion`) + `GroupRepost` records (kind:16 with `h`) — both are "things shared into the room library", projected together. The Room Library UI in `Communities/RoomLibrary*Card.swift` consumes this. |
| `GroupMembers` | `(host_relay_url, group_id)` | Members + admins. Hydration with NIP-01 profiles is a cross-crate join performed in `highlighter-core` per §6. |
| `GroupExplorer` | `(host_relay_url, optional filter)` | List of all publicly-discoverable groups (39000 events without the `hidden` marker) on a given host relay. |

### 3.3 `ActionModule` impls (13)

These are the writes. Each is an admin- or user-initiated event that NMP's `PublishPlanner` (M2 §7) routes per the host-relay-pin contract. Every action below carries a typed `nmp-nip29::GroupId { host_relay_url, local_id }` input so the publisher never has to derive the host from a bare string `h` value.

| `ActionModule` | Emits kind(s) | Auth | Routing |
|---|---|---|---|
| `CreateGroup` | 9007 + 9002 back-to-back | signer = founder | Host relay only. |
| `JoinRequest` | 9021 (optional `code` tag) | signer = requester | Host relay only. |
| `LeaveRequest` | 9022 | signer = leaver | Host relay only. |
| `EditMetadata` | 9002 | signer must be in 39001 | Host relay only. |
| `PutUser` | 9000 (optional `role` tag) | signer must be in 39001 | Host relay only. |
| `RemoveUser` | 9001 | signer must be in 39001 | Host relay only. |
| `CreateInvite` | 9009 (multi-fan-out per `MAX_CODES_PER_INVITE_EVENT`) | signer must be in 39001 | Host relay only. |
| `DeleteEvent` | 9005 | signer must be in 39001 | Host relay only. |
| `DeleteGroup` | 9008 | signer must be in 39001 (and ideally the original 9007 signer; see `kinds.md` §2.3) | Host relay only. |
| `PostChatMessage` | 9 with `h` | signer = author (must be in 39002 if `restricted`) | Host relay only. |
| `PostDiscussion` | 11 with `h` + `["t","discussion"]` | same as above | Host relay only. |
| `PostArtifact` | 11 with `h` and *no* `t=discussion` marker, carrying catalog reference tags per `kinds.md` §2.1 | same as above | Host relay only. |
| `ShareEventIntoGroup` | 16 with `h` referencing an existing event (by `e` tag) | signer = re-sharer (in 39002 if `restricted`) | Host relay only. **The second write of the `publish-and-share` composed flow** described in `routing.md` §6; the *first* write (the kind:9802 highlight itself) lives in `nmp-nip84`. |

### 3.4 What `nmp-nip29` does **not** ship

- **No `CapabilityModule`.** The crate uses existing capabilities (signer, http, blossom for picture uploads) but doesn't add any.
- **No `IdentityModule`.** Groups don't change the user's identity model; M6/M8 covers identity.
- **No new persistence schema.** Per ADR-0010 + M3, all domain records persist via the kernel's LMDB tables keyed by their composite keys. The crate declares migrations in standard NMP shape.

**Total: 3 trait families touched (Domain, View, Action), 29 module impls total** (9 + 7 + 13). This is the return-statistic for the design pass.

## 4. The load-bearing constraint: host-relay-pin

The single most important property of NIP-29 — and the property every other design decision in this crate falls out of — is that **group operations bypass NIP-65 routing and pin to the group's host relay**.

This contradicts the M2 outbox planner's default behavior, which routes:

- subscriptions with `authors` → those authors' write relays
- subscriptions with `#p` → those pubkeys' read relays
- publishes → the author's write relays + recipient inbox relays for tag-targeted recipients

NIP-29 needs a **third routing lane**: the `h`-tag lane. Any filter with `#h: [group_id]` routes to the host relay; any publish of an event with an `h` tag routes to the host relay. The author's NIP-65 mailboxes don't enter into it.

Full design of how this lands in the compiler and publisher is in [`nip29/routing.md`](nip29/routing.md). The summary is: `nmp-nip29` declares its interests as a typed `RelayPinnedInterest` (carries `host_relay_url` explicitly), and the M2 compiler short-circuits its three-lane logic when it sees that variant.

## 5. The "group identity" type

A `GroupId` in `nmp-nip29` is **not** a bare string. It is:

```rust
pub struct GroupId {
    pub host_relay_url: RelayUrl,   // normalized per NIP-65 url-canonicalization
    pub local_id: String,           // matches NIP-29 charset [a-z0-9-_]+
}
```

This is the breaking change versus Highlighter's existing core, which treats `group_id` as a `String` because there's only ever one host. Every `DomainRecord`, every `ViewModule`'s composite key, every `ActionModule`'s input that references a group uses `GroupId`. The kernel's composite-key reverse index (ADR-0001) handles `(host_relay_url, local_id)` as cleanly as it handles `(pubkey, kind)` — no kernel changes needed; the change is purely at the crate boundary.

For UI surfaces that need a flat string (URLs, deep links, share cards), `nmp-nip29::GroupId` provides `to_uri()` / `from_uri()` round-tripping into the NIP-29 spec format `<host>'<local-id>` (e.g. `groups.nostr.com'abcdef`).

## 6. Cross-crate joins (resolved at the app layer, not inside `nmp-nip29`)

The user surfaces the UI consumes need joins against three other crates. Per the M11.5 exit gate (`nmp-nip29` must not import any other `nmp-nip*` crate), the joins live **in `highlighter-core`** (the app's own extension crate), where they compose `nmp-nip29`'s views with views from other protocol crates using only the kernel-level composite-key reverse index:

| Composed view (lives in `highlighter-core`) | Composes | Mechanism |
|---|---|---|
| `HydratedGroupChat` | `nmp-nip29::GroupChat` + `nmp-nip01::Profile` for each author | composite-key dependency tracking at the substrate level (ADR-0001) — `highlighter-core::HydratedGroupChat::dependencies()` enumerates both; the kernel reverse-index handles the join with no protocol-crate awareness |
| `DiscussionsWithReplyCounts` | `nmp-nip29::GroupDiscussions` + `nmp-nip22::Comment { e: <discussion_id> }` per discussion root | same pattern; the comment count + latest-reply ordering happens in `highlighter-core`'s `project()` |
| `GroupArtifactLanes` | `nmp-nip29::GroupHome` (which surfaces the kind:16 reposts tagged with the room's `h`) + `nmp-nip84::Highlight` deref'd via the repost's `e` tag + the original artifact via the highlight's reference | same pattern; the deref chain happens in `highlighter-core`'s projection |

Why this works: the kernel's composite-key reverse index (`crates/nmp-core/src/substrate/view.rs::ViewDependencies`) is generic — it doesn't care which crate owns which DomainModule. An app-level ViewModule can declare dependencies on records owned by any protocol crate the app has registered. The protocol crates stay protocol-only; cross-protocol composition is the app's job.

`nmp-nip29` ships its own non-hydrated views (`GroupChat`, `GroupDiscussions`, `GroupHome`, etc.) that are useful on their own (debugging UIs, tests, headless clients) without any cross-crate joins. The hydrated variants are app-level conveniences.

This is also why kind:16 (generic repost) gets its own home: if it lives in `nmp-nip18`, `nmp-nip29` doesn't import it; the cross-protocol "list artifacts shared into this group" view is composed in `highlighter-core`. If `nmp-nip18` doesn't yet exist when M11.5 starts, the simplest interim is to put kind:16 ingest in `highlighter-core` itself; the ADR-0011 RelayPinnedInterest still routes correctly because the `h` tag is what matters, not which crate owns the kind.

## 7. What's deferred vs in-scope

**In M11.5 scope:**

- All 9 `DomainModule` impls per §3.1
- All 7 `ViewModule` impls per §3.2 (with host-relay-pin routing fully wired through the M2 compiler)
- All 13 `ActionModule` impls per §3.3 (with host-relay-pin routing fully wired through the M2 publish planner)
- Full ingest pipeline: 39000–39003 trusted because they're relay-signed; 9000–9022 audited into the moderation trail; `previous`-tag validation per `moderation.md`
- Highlighter UI parity for every NIP-29-bearing and NIP-29-adjacent feature in `feature-inventory.md` §§ 1–2

**Deferred to a follow-up milestone or to relay-side implementation:**

- **Group migration** (NIP-29 supports groups moving between relays; UI for "this group moved, follow it to its new host?" is post-M11.5).
- **Group forking** (same `local_id` on two relays = two groups; UI to disambiguate post-M11.5).
- **Relay-keypair trust pinning** (today we trust whatever pubkey signs the 39000 on the host relay; per-relay pubkey pinning would be a NIP-11-driven extension, not blocking M11.5).
- **`hidden` group support** (a metadata flag that hides the 39000 from non-members; we recognize the flag in projection but don't ship a UI for hidden-group invites in M11.5).

## 8. Open questions for follow-up ADRs

1. **Where does the host-relay-pin routing rule live in the planner?** Two viable shapes: (a) `nmp-nip29` returns a typed `RelayPinnedInterest` that the compiler's outer dispatch handles, vs (b) the compiler grows a generic "honor pin-hints from any crate" mechanism and `nmp-nip29` participates via a trait. (b) is cleaner long-term (other relay-pinned NIPs may emerge) but (a) ships M11.5 faster. ADR needed.
2. **Trust model for relay-signed metadata.** Today we accept any 39000 from the host relay. If the host relay rotates keys mid-life, do we re-fetch or accept the new pubkey silently? See `moderation.md` §4 for the dimensions; ADR needed for the policy.
3. **`JoinedGroups` aggregation across multiple host relays.** A user may be in groups on `groups.0xchat.com` + `relay.highlighter.com` + `relay29.fiatjaf.com` simultaneously. The view runs against the union; the M2 compiler must produce one plan per host relay. Confirm in an ADR that the planner handles this without a per-crate scatter-gather helper.
4. **Membership-as-security-boundary in projections.** A private group's chat is read-restricted to members. Today we render whatever 39002 says we are; if 39002 hasn't arrived yet we may render nothing. Do we explicitly *gate* the projection on a known-membership state, or do we let best-effort-rendering (D1) handle the empty case naturally? Bias: D1.
5. **Invite-code redemption UX vs JoinRequest.** The 9021 with a `code` tag is the redemption path. The current Highlighter onboarding lets the user paste a code before any signer is installed. Does the redemption action defer until a signer exists, or do we mint a fresh local key and redeem immediately? Cross-cuts M6 (signer flows). ADR-level question.
6. **Tombstoning on kind:9008 (delete-group).** The relay can delete a group entirely. What does the kernel do with all the DomainRecords keyed under `(host_relay_url, group_id)`? Bias: hard-delete the records (the relay no longer serves them; provenance dies with the group), surface a one-shot "group deleted" notification through the diagnostics lane.

The three sub-files (`nip29/routing.md`, `nip29/kinds.md`, `nip29/moderation.md`) work through these in detail.
