# `nmp-nip29` — Event Kinds Catalog

> Sub-file of [`../nip29-crate.md`](../nip29-crate.md). Exhaustive map of every NIP-29 event kind, its required + optional tags, its origin (user vs admin vs relay), and which `nmp-nip29` module owns ingest + projection.
> **Source of truth:** NIP-29 spec at `https://github.com/nostr-protocol/nips/blob/master/29.md` (fetched 2026-05-18).

## 1. Conceptual split: three event-kind classes

NIP-29 segregates kinds into three populations by *signer authority* and *routing*:

| Class | Kind range | Signer | Routing | Replaceable? |
|---|---|---|---|---|
| **User-sent group events** | any kind with an `h` tag (incl. 9, 11, plus arbitrary kinds the group permits) | the human user | host relay (pin) | per-kind (kind:9 chat is regular; kind:11 discussion is regular; future kinds may differ) |
| **User management** | 9021, 9022 | the human user | host relay (pin) | regular (audit trail) |
| **Moderation** | 9000–9009 | a current admin | host relay (pin) | regular (audit trail) |
| **Group metadata** | 39000–39003 | the **relay** | host relay (pin) — only ever exists there | parameterized-replaceable on `d = group_id` |

This is the structure the `nmp-nip29` protocol records, actions, and projections
follow. The kernel's generic ingest pipeline dispatches accepted events to
`nmp-nip29`'s ingest/read-model hooks; those hooks do the NIP-29-specific work:
structural validation, audit-trail materialisation, and routing of unknown
`h`-tagged kinds to the `GroupContextEvent` fallback. Authority validation for
admin actions is a publish-time action concern, not an ingest-time concern; see
`moderation.md` §3.

## 2. Full catalog

### 2.1 User-sent group events

#### Kind 9 — Group chat message

- **Required tags:** `["h", <group_id>]`
- **Optional tags:** `["e", <reply-target-id>, "", "reply"]` (NIP-10-style reply marker); `["e", <root-id>, "", "root"]` for deeply-nested replies; `["p", <mentioned-pubkey>]` per mention
- **Content:** the message body, free-form text
- **Routing:** host relay (pin)
- **Owner:** `nmp-nip29::GroupChatMessage` record/projection; surfaced by `GroupChat`
- **Replaceable:** no
- **Notes:** Highlighter's `chat.rs` is the reference impl. Empty content rejected by the framework at write time; NIP-29 itself doesn't ban it but it's a UX rule.

#### Kind 11 — Two-variant dispatch (Highlighter convention)

Highlighter overloads kind:11 as **two distinct event shapes** with the same wire kind, discriminated structurally by the presence of `["t","discussion"]`:

**Kind 11 — Group discussion** (with `["t","discussion"]`)

- **Required tags:** `["h", <group_id>]`, `["t", "discussion"]`
- **Optional tags:** `["title", <discussion title>]`; `["image", <url>]` per attached image; `["alt", <accessibility text>]`
- **Content:** the discussion body (markdown supported)
- **Routing:** host relay (pin)
- **Owner:** `nmp-nip29::GroupDiscussion` record/projection; surfaced by `GroupDiscussions`
- **Replaceable:** no
- **Emitted by:** the app/component layer shapes the kind:11 discussion event (content + `t`/`title`/`image` tags) and hands it to the single kind-agnostic `nmp.nip29.publish_group_event` write surface; `nmp-nip29` injects only the `h`/`previous`/host-pin envelope (#2513). NIP-29 ships no per-kind `PostDiscussion` action.
- **Notes:** Discussion replies in Highlighter today are NIP-22 kind:1111 comments scoped via `E`/`e` tags to the discussion event — **they do NOT carry an `h` tag** (verified against `app/core/src/comments.rs::publish_comment` and `Communities/DiscussionDetailView.swift::artifactRef = .event(id: discussion.eventId, kind: 11)`). Per the unifying rule in §4, that makes them ordinary `nmp-nip22::Comment` records (not `nmp-nip29::GroupComment`), routed per the author's NIP-65 write relays — *not* host-pinned. The discussion view's reply-thread join is a cross-crate composition done in `highlighter-core` (per `nip29-crate.md` §6's `DiscussionsWithReplyCounts`), reading from `nmp-nip22`'s public comment stream filtered to the discussion's event id. **M11.5 must preserve this behavior** to keep historical replies visible and to match the copied UI's publish path. (A future iteration could add an `h` tag to in-room comments to make them group-private, but that's a Highlighter UX decision, not an M11.5 deliverable.) The `t=discussion` marker is recognised by both Highlighter and 0xchat-style clients but is NOT in the NIP-29 spec; document the convention in the M11.5 exit-gate report and consider proposing it upstream.

**Kind 11 — Group artifact share** (without `["t","discussion"]`, with catalog tags)

- **Required tags:** `["h", <group_id>]`, `["d", <artifact_id>]` (Highlighter convention: a stable artifact identifier per `artifacts.rs::artifact_id_from_reference_key`), plus *one* of the catalog reference tags:
  - `["r", <url>]` for articles + podcast episodes + web bookmarks
  - `["i", <isbn-or-other-identifier>]` for books
  - `["a", <30023:pubkey:d>]` for NIP-23 long-form references
- **Optional tags:** `["title", …]`, `["image", …]`, `["alt", …]`, podcast-specific `["chapter", …]` arrays (per Highlighter's lift-podcast-tags convention), `["preview-audio", …]`
- **Content:** an optional user note about why this artifact is shared
- **Routing:** host relay (pin)
- **Owner:** `nmp-nip29::GroupArtifact` record/projection; surfaced by `GroupArtifacts` (the Room Library lanes)
- **Replaceable:** **No** — kind:11 is a regular Nostr event kind (not 30000-39999), so NIP-33 parameterized replacement does NOT apply. The Highlighter relay performs a *custom* upsert by `d` tag (per `artifacts.rs::publish` comment "if a duplicate kind:11 with the same `d` tag exists the relay will upsert"), but this is **relay-specific behavior**, not a Nostr protocol guarantee. `nmp-nip29`'s storage keys `GroupArtifact` by `event_id` (regular event semantics); any `d`-based dedup in projections is documented as relay-specific and applied at the projection layer, not at storage.
- **Emitted by:** the app/component layer shapes the kind:11 artifact event (content + `d`/catalog tags) for the "Suggest an artifact to the room" flow and hands it to the single kind-agnostic `nmp.nip29.publish_group_event` write surface; `nmp-nip29` injects only the `h`/`previous`/host-pin envelope (#2513). NIP-29 ships no per-kind `PostArtifact` action.
- **Notes:** This is *Highlighter convention*, layered on the same NIP-29 routing as the discussion variant. The `t=discussion` absence + the presence of a catalog tag is the structural discriminator. `nmp-nip29` ships ingest for both shapes because the dispatch is wire-level structural; apps that don't want the artifact-share path simply don't consume the `GroupArtifacts` view.

#### Future / extensibility

NIP-29 explicitly allows **any kind** with an `h` tag to be a group event. The
`nmp-nip29` ingest path therefore checks for `h` first and routes ingest to the
group context, then dispatches by kind to the owning protocol/app handler if
one exists. Unknown kinds with `h` are stored as generic `GroupContextEvent`
records so apps that ship custom group event kinds can layer their own
projection/action logic without modifying `nmp-nip29`.

### 2.2 User management

#### Kind 9021 — Join request

- **Required tags:** `["h", <group_id>]`
- **Optional tags:** `["code", <invite_code>]` for preauthorized join; `["e", <referrer-event-id>]` for "who invited me" tracing
- **Content:** optional human-readable reason for joining
- **Signer:** the prospective member
- **Routing:** host relay (pin)
- **Owner:** `nmp-nip29::GroupModerationEvent` audit/read model; emitted by `JoinRequest` ActionModule
- **Relay reaction:** open + uncoded → publish 39002 with new member. Closed + valid code → publish 39002 + consume code (single-use per Highlighter's notes; matches relay29 convention). Closed + no code → silently held for admin review, or rejected with a typed error per the relay's policy.
- **Notes:** the `code` tag mechanism is the same one used by `create-invite` (kind:9009) on the admin side. A relay accepting a 9021 with a `code` consumes the code from its store.

#### Kind 9022 — Leave request

- **Required tags:** `["h", <group_id>]`
- **Optional tags:** none in current NIP-29
- **Content:** optional human-readable reason
- **Signer:** the leaver
- **Routing:** host relay (pin)
- **Owner:** `nmp-nip29::GroupModerationEvent`; emitted by `LeaveRequest` ActionModule
- **Relay reaction:** publish a 9001 remove-user automatically; updated 39002 follows.
- **Notes:** Highlighter's existing `groups.rs` does **not** implement leave — `nmp-nip29` adds it.

### 2.3 Moderation (admin-signed, 9000–9009)

All require `["h", <group_id>]` and are signed by a current admin (member of the latest 39001) — **with one exception: kind 9007 (create-group)** has no admin requirement because it's the event that *establishes* the group; the signer becomes the founding admin, materialised as the initial 39001 the relay emits in response. Every other 9000-series kind is admin-only. The relay validates signer membership in 39001 *before* republishing the corresponding 39000/39001/39002.

#### Kind 9000 — Put user

- **Required tags:** `["h", <group_id>]`, `["p", <target_pubkey_hex>]` (or `["p", <target_pubkey_hex>, <role_name>]` to grant a role atomically with the membership change)
- **Optional tags:** `["reason", <text>]`. **Role tag format:** NIP-29 + relay29 parse roles from *extra elements on each `p` tag* — `["p", <pubkey>, <role>]` associates the role with the target. **Emit MUST use this form**; a sibling `["role", <name>]` tag (which earlier drafts of this doc suggested) is not associated with the target and will be ignored by relay29, causing the user to be added as a plain member without admin promotion. Ingest accepts both wire formats (per-p-tag role *and* a sibling `role` tag) for compatibility with other client conventions, normalizing both to the same in-memory shape.
- **Routing:** host relay (pin)
- **Effect:** target pubkey added to 39002 (and to 39001 if a role tag is present in either form).
- **Owner:** emitted by `PutUser` ActionModule.

#### Kind 9001 — Remove user

- **Required tags:** `["h", <group_id>]`, `["p", <target_pubkey_hex>]`
- **Optional:** `["reason", <text>]`
- **Effect:** target pubkey removed from 39002 (and from 39001 if previously present).
- **Owner:** emitted by `RemoveUser` ActionModule.

#### Kind 9002 — Edit metadata

- **Required:** `["h", <group_id>]`
- **Optional tags:** `["name", <text>]`, `["about", <text>]`, `["picture", <url>]`, `["public"]`/`["private"]`, `["open"]`/`["closed"]`, `["restricted"]`, `["hidden"]`
- **Effect:** relay republishes 39000 with the new values; absent tags retain their previous values.
- **Owner:** emitted by `EditMetadata` ActionModule.
- **Notes:** Highlighter's create-room flow also uses 9002 immediately after 9007 to set initial name/about/picture/visibility/access (`groups.rs::create_room` lines 308–335). The 9002 + 9007 sequence is a convention, not a spec rule; `nmp-nip29::CreateGroup` will encapsulate the pair.

#### Kind 9005 — Delete event

- **Required:** `["h", <group_id>]`, `["e", <target_event_id>]`
- **Effect:** relay removes the target event from its store and (per relay policy) refuses to redeliver it.
- **Owner:** emitted by `DeleteEvent` ActionModule.
- **Notes:** Highlighter does **not** ship moderation deletion today. `nmp-nip29` adds it; UI in M11.5 is a long-press → "remove" affordance on chat + discussion items, visible only to admins.

#### Kind 9007 — Create group

- **Required:** `["h", <group_id>]`
- **Content:** empty
- **Effect:** relay initialises the group with the signer as the founding admin; emits initial **39001 + 39002** (39003 is optional and many NIP-29 relays do not emit it; the `CreateGroup` action and exit-gate tests treat 39003 as best-effort, never blocking on it).
- **Owner:** emitted by `CreateGroup` ActionModule (which then emits the follow-up 9002 for metadata).

#### Kind 9008 — Delete group

- **Required:** `["h", <group_id>]`
- **Effect:** relay hard-deletes the group; tombstones the 39000–39003; refuses further events with that `h` tag.
- **Owner:** emitted by `DeleteGroup` ActionModule (admin-only; UI affordance in admin settings).
- **Notes:** **kernel-side response is hard-delete of all DomainRecords keyed under that GroupId**; surface as a one-shot diagnostic event so the UI can render "group deleted" toast. (Open question 6 in `../nip29-crate.md` §8.)

#### Kind 9009 — Create invite

- **Required:** `["h", <group_id>]`
- **Optional:** one or more `["code", <code_string>]` (Highlighter caps at 10 per event = `MAX_CODES_PER_INVITE_EVENT`; the `CreateInvite` action fan-outs across multiple 9009s for larger batches)
- **Effect:** relay records the codes as redeemable; each code is consumed on the first 9021 that uses it (single-use semantics per relay29).
- **Owner:** emitted by `CreateInvite` ActionModule.

### 2.4 Group metadata (relay-signed, parameterized-replaceable, 39000–39003)

These are the most unusual events in Nostr — they're *intended* to be signed by the relay's own keypair, not by any user. The kernel's normal "verify signature against author pubkey" path applies, plus a **mandatory metadata-signer trust check** per `moderation.md` §4.3: policy A (NIP-11 identity match) when NIP-11 declares a relay pubkey; policy B (TOFU on first-seen signer per `(host_relay_url, group_id)`) otherwise. **C (accept-any-from-host) is NOT shipped** — a host relay that also accepts ordinary parameterized events would otherwise let a malicious user spoof 39001/39002 and forge admin/membership state. See `moderation.md` §4 for the rationale and `routing.md` §4.3 for the related bootstrap-host signer match.

All four kinds share:

- **Required tag:** `["d", <group_id>]` (the parameterized-replaceable key per NIP-33)
- **Routing:** host relay only — these events never exist anywhere else
- **Signer:** the relay's keypair
- **Replaceable:** yes — each new event for the same `d` supersedes the previous

#### Kind 39000 — Group metadata

- **Optional tags:** `["name", <text>]`, `["picture", <url>]`, `["about", <text>]`, `["public"]`/`["private"]`, `["open"]`/`["closed"]`, `["restricted"]`, `["hidden"]`
- **Owner:** `nmp-nip29::Group` protocol record/projection
- **Defaults:** per Highlighter's reference, absence of `private`/`closed`/`hidden` tags defaults to public/open/visible (`groups.rs::build_summary` lines 469–476). `nmp-nip29` adopts the same defaults.

#### Kind 39001 — Group admins

- **Optional tags:** one or more `["p", <pubkey>]` (2-element form) or `["p", <pubkey>, <role_name>]` (3-element form for relays that publish 39003) or `["p", <pubkey>, <role_name>, <description>]`
- **Owner:** `nmp-nip29::GroupAdmins` protocol record/projection
- **Notes:** Highlighter ignores the optional role/description fields today and projects only the pubkey set. `nmp-nip29` preserves the 3rd + 4th elements in the DomainRecord so role-aware UIs can opt-in.

#### Kind 39002 — Group members

- **Optional tags:** one or more `["p", <pubkey>]`
- **Owner:** `nmp-nip29::GroupMembers` protocol record/projection
- **Notes:** For very large groups, 39002 may be sharded by the relay (NIP-29 mentions but does not standardize sharding). `nmp-nip29` does **not** support sharded 39002 in M11.5; we observe whichever 39002 the relay sends as ground truth. If sharding emerges as a real-world need, a follow-up milestone adds union semantics.

#### Kind 39003 — Group roles

- **Optional tags:** one or more `["role", <role_name>, <description>]` declaring the role names the relay knows about for this group
- **Owner:** `nmp-nip29::GroupRoles` protocol record/projection
- **Notes:** Optional in the NIP; many relays don't publish it. The view modules treat absence as "role-name strings on 39001 entries are decorative, not first-class".

## 3. Tag conventions across kinds

The `h` tag is the **routing key** for every user-sent and admin-signed group event; the `d` tag is the **replaceable key** for every relay-signed metadata event. They carry the *same* group_id value but in different slots — there is no "h tag on a 39000" (the 39000 uses `d` because it's parameterized-replaceable; using `h` would not enable replacement).

The `previous` tag is per-event optional; see `moderation.md` §2.

The `code` tag appears on both 9009 (mint side) and 9021 (redeem side).

The `p` tag is used in three distinct ways across this kind set:

- on 9000/9001: targets of user-targeting moderation actions
- on 39001/39002: enumerates the admin/member set
- on user-sent group events (kind 9, 11): mentions, NIP-10-style

The `e` tag carries the target for event-targeting moderation: kind 9005 (delete-event) uses `["e", <event_id>]`, not `["p", ...]`. The `ModerationEvent` parser must look for `target_event_id` (from `e`) for 9005 and `target_pubkey` (from `p`) for 9000/9001.

`nmp-nip29` ingest preserves all `p`-tag and `e`-tag bearing events in their
owning record; the projections/read models know which interpretation applies
in their context.

#### Kind 16 — Generic repost into a group

- **Required tags:** `["h", <group_id>]`, `["e", <reposted_event_id>]`
- **Optional tags:** `["p", <original_author_pubkey>]`, `["k", <reposted_kind_string>]` (typically `"9802"` for highlight reposts; per `highlights.rs::build_repost_event`)
- **Content:** typically empty; some clients embed the reposted event JSON, but Highlighter does not
- **Routing:** host relay (pin)
- **Owner:** `nmp-nip18` owns the kind:16 repost event. `nmp-nip29` does **not**
  own a repost record/projection — an `h`-tagged repost is read back through the
  kind-agnostic `GroupEventsProjection` like any other foreign kind.
- **Replaceable:** no
- **Emitted by:** the kind:16 event is constructed by `nmp-nip18` (NIP-18 owns the repost kind) and routed into the group through the single kind-agnostic `nmp.nip29.publish_group_event` write surface, which injects the `h`/`previous`/host-pin envelope. `nmp-nip29` does **not** ship a per-kind repost action (#2513).
- **Notes:** This is a NIP-18 generic repost, scoped into a group by the `h` tag. `nmp-nip29` is kind-blind transport: it owns the `h`-tag routing concern only, never the kind. Repost *construction* lives in `nmp-nip18`; the `h`-tagged event is ingested through the generic group-event read path (kinds 7/16/9802 classify as `KindClass::GroupEvent` in `kinds.rs`, not as nip29-native kinds).
  Any cross-protocol publish-and-share sequencing lives at the app layer (see
  `routing.md` §6), never inside `nmp-nip29`.

## 4. The unifying rule — kind-blind transport (#2509 / #2513)

**`nmp-nip29` is a kind-blind transport.** It owns exactly two things: the
group-routing envelope (the `["h", group_id]` tag plus the `["previous", …]`
chain and the host-relay pin) and the NIP-29 kind namespace itself (9000–9022
moderation/user-management + 39000–39003 metadata). It does **not** name,
classify, or own any event kind outside that namespace.

An `["h", group_id]` tag on a non-NIP-29 kind makes that event *routable into a
group* — `nmp-nip29` routes it and reads it back through the kind-agnostic
`GroupEventsProjection` (consumer-declared kinds). It does **not** make the kind
NIP-29's to own. The kind is built and owned by its owning NIP, never by
`nmp-nip29`:

- **kind:7 (reaction)** is owned by `nmp-nip25` — `nmp-nip25` builds the reaction
  event (with the host-pin `e`/`p` tags it needs); the app adds the `["h", …]`
  tag and routes it through `nmp-nip29`'s generic write surface.
- **kind:16 (generic repost)** is owned by `nmp-nip18` — same pattern.
- **kind:1111 (NIP-22 comment)** is owned by `nmp-replies` / `nmp-nip22` (public
  reply/comment policy and kind:1111 mechanics).
- **kind:11 (discussion / artifact share)** is owned by the app layer (or a
  future content NIP); `nmp-nip29` carries it as opaque payload.
- **kind:9 (chat), kind:1 (text note), and any custom/future kind** (livestreams,
  polls, files) are likewise opaque to `nmp-nip29`.

Protocol-crate isolation stays intact: `nmp-nip25` knows nothing about groups (it builds a kind:7/kind:5 event); `nmp-nip29` knows nothing about reaction/repost semantics (it injects an envelope onto a caller-supplied event of any kind).

### The single write surface

There is exactly one group-event publish action: the generic
**`PublishGroupEventAction`** (`nmp.nip29.publish_group_event`). It takes a
caller-built `(kind, content, tags)` and injects **only** the envelope (`h` /
`previous` / host-pin). There are **no** per-kind named group actions — a
`ReactInGroup` / `RepostInGroup` / `ShareEventInGroup` / `CommentInGroup` would
re-assert kind ownership inside the kind-blind transport and is a doctrine
violation (the `nip29_kind_blind` doctrine-lint rule enforces this: only the
allowlisted lifecycle/admin/envelope verbs may appear as an `nmp.nip29.*`
namespace, and the `REACTION_KIND` / `REPOST_KIND` authoring constants are
banned from the crate).

So a viewer reacting in a group: `nmp-nip25` builds the kind:7 event → the app
adds `["h", group_id]` and the target `e`/`p` tags → dispatch
`nmp.nip29.publish_group_event`. A repost: `nmp-nip18` builds the kind:16 →
same routing. Cross-protocol *sequencing* (e.g. publish-and-share) happens at
the app layer, never inside `nmp-nip29`.

This keeps protocol-crate isolation intact in both directions: `nmp-nip25`
knows nothing about groups; `nmp-nip29` knows nothing about reactions, reposts,
comments, or any other foreign kind.
