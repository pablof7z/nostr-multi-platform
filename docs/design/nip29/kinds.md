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

This is the structure the `DomainModule` impls follow (per `../nip29-crate.md` §3.1). The kernel's ingest path validates signer authority *before* the module sees the event; see `moderation.md` for the validation rules.

## 2. Full catalog

### 2.1 User-sent group events

#### Kind 9 — Group chat message

- **Required tags:** `["h", <group_id>]`
- **Optional tags:** `["e", <reply-target-id>, "", "reply"]` (NIP-10-style reply marker); `["e", <root-id>, "", "root"]` for deeply-nested replies; `["p", <mentioned-pubkey>]` per mention
- **Content:** the message body, free-form text
- **Routing:** host relay (pin)
- **Owner:** `nmp-nip29::GroupChatMessage` DomainModule; projected by `GroupChat` ViewModule
- **Replaceable:** no
- **Notes:** Highlighter's `chat.rs` is the reference impl. Empty content rejected by the framework at write time; NIP-29 itself doesn't ban it but it's a UX rule.

#### Kind 11 — Two-variant dispatch (Highlighter convention)

Highlighter overloads kind:11 as **two distinct event shapes** with the same wire kind, discriminated structurally by the presence of `["t","discussion"]`:

**Kind 11 — Group discussion** (with `["t","discussion"]`)

- **Required tags:** `["h", <group_id>]`, `["t", "discussion"]`
- **Optional tags:** `["title", <discussion title>]`; `["image", <url>]` per attached image; `["alt", <accessibility text>]`
- **Content:** the discussion body (markdown supported)
- **Routing:** host relay (pin)
- **Owner:** `nmp-nip29::GroupDiscussion` DomainModule; projected by `GroupDiscussions` ViewModule
- **Replaceable:** no
- **Emitted by:** `PostDiscussion` ActionModule
- **Notes:** Replies on a discussion that carry an `["h", group_id]` tag are `nmp-nip29::GroupComment` records per §4's unifying ownership rule (every h-tagged event is `nmp-nip29`'s, regardless of kind). Only kind:1111 replies *without* an `h` tag fall to `nmp-nip22`. In practice, Highlighter's discussion-reply composer always attaches the room's `h` tag, so in-room replies are NIP-29-routed end-to-end. The `t=discussion` marker is recognised by both Highlighter and 0xchat-style clients but is NOT in the NIP-29 spec; document the convention in the M11.5 exit-gate report and consider proposing it upstream.

**Kind 11 — Group artifact share** (without `["t","discussion"]`, with catalog tags)

- **Required tags:** `["h", <group_id>]`, `["d", <artifact_id>]` (Highlighter convention: a stable artifact identifier per `artifacts.rs::artifact_id_from_reference_key`), plus *one* of the catalog reference tags:
  - `["r", <url>]` for articles + podcast episodes + web bookmarks
  - `["i", <isbn-or-other-identifier>]` for books
  - `["a", <30023:pubkey:d>]` for NIP-23 long-form references
- **Optional tags:** `["title", …]`, `["image", …]`, `["alt", …]`, podcast-specific `["chapter", …]` arrays (per Highlighter's lift-podcast-tags convention), `["preview-audio", …]`
- **Content:** an optional user note about why this artifact is shared
- **Routing:** host relay (pin)
- **Owner:** `nmp-nip29::GroupArtifact` DomainModule; projected by `GroupArtifacts` ViewModule (the Room Library lanes)
- **Replaceable:** by `d` tag (the relay upserts a duplicate share with the same `d`); per Highlighter's `artifacts.rs::publish` comment "if a duplicate kind:11 with the same `d` tag exists the relay will upsert".
- **Emitted by:** `PostArtifact` ActionModule (the "Suggest an artifact to the room" flow)
- **Notes:** This is *Highlighter convention*, layered on the same NIP-29 routing as the discussion variant. The `t=discussion` absence + the presence of a catalog tag is the structural discriminator. `nmp-nip29` ships ingest for both shapes because the dispatch is wire-level structural; apps that don't want the artifact-share path simply don't consume the `GroupArtifacts` view.

#### Future / extensibility

NIP-29 explicitly allows **any kind** with an `h` tag to be a group event. The `nmp-nip29` ingest path therefore checks for `h` first and routes ingest *to the group context*, then dispatches by kind to the owning DomainModule (if any). Unknown kinds with `h` are stored in a generic `GroupContextEvent` DomainRecord so apps that ship custom group event kinds (livestreams, polls, files) can layer their own DomainModules without modifying `nmp-nip29`.

### 2.2 User management

#### Kind 9021 — Join request

- **Required tags:** `["h", <group_id>]`
- **Optional tags:** `["code", <invite_code>]` for preauthorized join; `["e", <referrer-event-id>]` for "who invited me" tracing
- **Content:** optional human-readable reason for joining
- **Signer:** the prospective member
- **Routing:** host relay (pin)
- **Owner:** `nmp-nip29::GroupModerationEvent` DomainModule (audit trail); emitted by `JoinRequest` ActionModule
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

All require `["h", <group_id>]` and are signed by a current admin (member of the latest 39001). The relay validates signer membership in 39001 *before* republishing the corresponding 39000/39001/39002.

#### Kind 9000 — Put user

- **Required tags:** `["h", <group_id>]`, `["p", <target_pubkey_hex>]`
- **Optional tags:** `["role", <role_name>]` (sibling tag, Highlighter convention — `groups.rs::create_invite_codes` neighbours and `nip29-crate.md` §3.3 both use the sibling-tag form `["role","admin"]`); `["reason", <text>]`. **Note:** The NIP-29 spec also allows the role name as a third element on the `p` tag (`["p", <pubkey>, <role_name>]`); for emit, we use the sibling `role` tag form because it matches Highlighter's existing client and relay29's expectations, but ingest accepts both wire formats and normalizes to the same in-memory shape.
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
- **Effect:** relay initialises the group with the signer as the founding admin; emits initial 39001 + 39002 + 39003.
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

These are the most unusual events in Nostr — they're signed by the relay's own keypair, not by any user. The kernel's normal "verify signature against author pubkey" path applies, but the *authority* check is "does the signer pubkey match the host relay's declared identity?" (per NIP-11 `pubkey` or by pinned trust; see `moderation.md` §4).

All four kinds share:

- **Required tag:** `["d", <group_id>]` (the parameterized-replaceable key per NIP-33)
- **Routing:** host relay only — these events never exist anywhere else
- **Signer:** the relay's keypair
- **Replaceable:** yes — each new event for the same `d` supersedes the previous

#### Kind 39000 — Group metadata

- **Optional tags:** `["name", <text>]`, `["picture", <url>]`, `["about", <text>]`, `["public"]`/`["private"]`, `["open"]`/`["closed"]`, `["restricted"]`, `["hidden"]`
- **Owner:** `nmp-nip29::Group` DomainModule
- **Defaults:** per Highlighter's reference, absence of `private`/`closed`/`hidden` tags defaults to public/open/visible (`groups.rs::build_summary` lines 469–476). `nmp-nip29` adopts the same defaults.

#### Kind 39001 — Group admins

- **Optional tags:** one or more `["p", <pubkey>]` (2-element form) or `["p", <pubkey>, <role_name>]` (3-element form for relays that publish 39003) or `["p", <pubkey>, <role_name>, <description>]`
- **Owner:** `nmp-nip29::GroupAdmins` DomainModule
- **Notes:** Highlighter ignores the optional role/description fields today and projects only the pubkey set. `nmp-nip29` preserves the 3rd + 4th elements in the DomainRecord so role-aware UIs can opt-in.

#### Kind 39002 — Group members

- **Optional tags:** one or more `["p", <pubkey>]`
- **Owner:** `nmp-nip29::GroupMembers` DomainModule
- **Notes:** For very large groups, 39002 may be sharded by the relay (NIP-29 mentions but does not standardize sharding). `nmp-nip29` does **not** support sharded 39002 in M11.5; we observe whichever 39002 the relay sends as ground truth. If sharding emerges as a real-world need, a follow-up milestone adds union semantics.

#### Kind 39003 — Group roles

- **Optional tags:** one or more `["role", <role_name>, <description>]` declaring the role names the relay knows about for this group
- **Owner:** `nmp-nip29::GroupRoles` DomainModule
- **Notes:** Optional in the NIP; many relays don't publish it. The view modules treat absence as "role-name strings on 39001 entries are decorative, not first-class".

## 3. Tag conventions across kinds

The `h` tag is the **routing key** for every user-sent and admin-signed group event; the `d` tag is the **replaceable key** for every relay-signed metadata event. They carry the *same* group_id value but in different slots — there is no "h tag on a 39000" (the 39000 uses `d` because it's parameterized-replaceable; using `h` would not enable replacement).

The `previous` tag is per-event optional; see `moderation.md` §2.

The `code` tag appears on both 9009 (mint side) and 9021 (redeem side).

The `p` tag is used in four distinct ways across this kind set:

- on 9000/9001/9005: targets of moderation actions
- on 39001/39002: enumerates the admin/member set
- on user-sent group events (kind 9, 11): mentions, NIP-10-style

`nmp-nip29` ingest preserves all `p`-tag-bearing events in their owning DomainRecord; the View modules know which interpretation applies in their context.

#### Kind 16 — Generic repost into a group

- **Required tags:** `["h", <group_id>]`, `["e", <reposted_event_id>]`
- **Optional tags:** `["p", <original_author_pubkey>]`, `["k", <reposted_kind_string>]` (typically `"9802"` for highlight reposts; per `highlights.rs::build_repost_event`)
- **Content:** typically empty; some clients embed the reposted event JSON, but Highlighter does not
- **Routing:** host relay (pin)
- **Owner:** `nmp-nip29::GroupRepost` DomainModule; surfaced inside `GroupArtifacts` ViewModule
- **Replaceable:** no
- **Emitted by:** `ShareEventIntoGroup` ActionModule; also the second leg of the `publish-and-share` composed flow described in `routing.md` §6
- **Notes:** This is NIP-18 generic repost, scoped into a group by the `h` tag. `nmp-nip29` owns this DomainModule in M11.5 because (a) no separate `nmp-nip18` crate exists yet, (b) the routing concern is the `h` tag, not the kind, (c) the surface is one domain + one action. A future `nmp-nip18` extraction would lift the *non-`h`* repost case out, leaving `nmp-nip29` owning only the `h`-tagged variant.

## 4. The unifying ownership rule

**Any event carrying an `["h", group_id]` tag is a NIP-29 group event and lives in `nmp-nip29`, regardless of its kind.** The `h` tag is the ownership discriminator; the kind is just the dispatch.

This applies to:

- **kind:7 (reaction) with an `h` tag** → `GroupReaction` DomainModule + `ReactInGroup` ActionModule in `nmp-nip29`. The non-`h` (public) reaction stays in `nmp-nip25`.
- **kind:1111 (NIP-22 comment) with an `h` tag** → `GroupComment` DomainModule + `CommentInGroup` ActionModule in `nmp-nip29`. The non-`h` (public) comment stays in `nmp-nip22`.
- **kind:16 (generic repost) with an `h` tag** → `GroupRepost` DomainModule + `ShareEventIntoGroup` ActionModule in `nmp-nip29` (per §2.2). The non-`h` repost would live in a future `nmp-nip18`.
- **kind:11 with `h`** (both `t=discussion` and artifact-share variants) → `nmp-nip29` per §2.1.
- **kind:9 with `h`** → `nmp-nip29` per §2.1.

The "owned by another crate" pattern still applies to the non-`h` variants of these kinds (`nmp-nip25` for public reactions, `nmp-nip22` for public comments, future `nmp-nip18` for public reposts), keeping protocol-crate isolation intact: `nmp-nip25` knows nothing about groups; `nmp-nip29` knows nothing about public reactions.

The only kind we explicitly **don't** model in `nmp-nip29` despite being able to carry an `h` tag:

- **kind:1 (text note) with an `h` tag.** Some relays accept kind:1 inside groups; we treat that as kind:9-equivalent for projection but do not actively emit kind:1 from `nmp-nip29` actions. UI rendering: same as chat. Kept out of `nmp-nip29`'s action surface to avoid ambiguity with public kind:1 owned by the app's social crate; ingest is best-effort.

**Custom kinds with `h` tags** (livestreams, polls, files, future NIPs) follow the generic `GroupContextEvent` fallback per §2.1's "Future / extensibility" note — apps that ship custom group event kinds layer their own DomainModules without modifying `nmp-nip29`, because the `h`-tag-is-the-ownership rule is structural and works for any kind.
