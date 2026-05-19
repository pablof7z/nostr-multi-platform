# `nmp-nip29` — Moderation, Trust, and the `previous`-Tag Chain

> Sub-file of [`../nip29-crate.md`](../nip29-crate.md). Covers the ingest-time validation rules that make NIP-29's moderation guarantees real: the relay-signed metadata trust check, the `previous`-tag forgery-prevention chain, and the audit trail the framework keeps for admin actions.
> **Companion:** [`kinds.md`](./kinds.md) (the catalog of moderation kinds 9000–9009).

## 1. Trust model in one paragraph

NIP-29 is **relay-trustful by design**. Group identity (39000), admin set (39001), member set (39002), and the role catalog (39003) are signed by the relay, not by any user. Moderation actions (9000–9009) are signed by users (admins), but their *authority* comes from being in the relay's 39001 — and the *only* attestation of who's in 39001 is the relay's signature. **The host relay is the trust anchor**; if you don't trust the host relay, you don't have a group. This is not a flaw — it's the central design choice. It buys cheap, simple, censorship-resistant-from-the-rest-of-Nostr group hosting at the cost of relay-level trust. NMP must respect this property faithfully, not paper over it.

## 2. The `previous`-tag chain

Per NIP-29 §"Timeline References":

> To prevent message misuse across forked groups, clients attach `previous` tags referencing event IDs from the last 50 events. Relays reject events with broken timeline references, enforcing contextual integrity.

The mechanism:

- When a client sends a group event, it includes one or more `["previous", <first-8-chars-of-event-id>, …]` tags referencing event IDs from the last 50 events the client has observed in the group.
- The relay validates: do those `previous` references match events the relay has in its store for this group? If yes, accept. If any reference is *wrong* (doesn't match the relay's view of the group's recent timeline), reject — the event was forged or replayed from a different fork of the group.
- The spec recommends *at least 3* references per event.

### 2.1 What this means for `nmp-nip29`

This is **an outbound-message responsibility**, handled in the `ActionModule::dispatch` path before publish:

1. Every action that emits a user-sent group event or moderation event consults the group's recent-event cache (kept by `nmp-nip29`, populated by the kernel's ingest from group-scoped subscriptions).
2. The action attaches `["previous", <id_prefix_1>, <id_prefix_2>, …]` tags with at least 3 references from the cached recent events, picked by the strategy "newest first, no duplicates".
3. If the cache is cold (the client just opened the group with no recent ingest yet), the action delays publishing until either (a) the cache populates from a freshly-issued REQ targeting the last 50 events, or (b) a timeout fires and the publish proceeds without `previous` tags. The relay's policy for missing-previous-tags determines whether (b) succeeds; many relays accept it for cold-start scenarios.

### 2.2 Ingest-side responsibility

`nmp-nip29`'s ingest does **not** re-validate the `previous` chain — the relay already did that and rejected anything that failed. The client trusts the relay's validation. (If we ran our own relay-validation logic, we'd be re-implementing the relay; we'd also be vulnerable to disagreeing with the relay about which events count, which is what `previous` is designed to anchor.) We **do** preserve the `previous` tags on ingested events in the DomainRecord for forensic / audit UI later.

### 2.3 Cache size + eviction

The recent-event cache per group is bounded at 50 entries (the spec's reference window). LRU eviction by `created_at`. The cache lives in `nmp-nip29::RecentGroupEvents` keyed by `GroupId`. Persistence is best-effort (M3 LMDB) — losing it costs a cold-start delay, no correctness loss.

### 2.4 Failure mode the user must see

If a relay rejects an outbound group event with a `previous`-mismatch error, the action's status transitions to `ActionStatus::Rejected { reason: PreviousChainBroken }`. The UI surfaces "this group's view is stale, try refreshing". This is a recoverable error — refreshing the cache from the relay and re-trying the publish almost always succeeds. The diagnostics lane records the chain mismatch for the developer to inspect.

## 3. Authority validation: admin-signed actions

The relay validates 9000–9009 authority before republishing the resulting 39001/39002. But `nmp-nip29` **also pre-validates** in the action dispatch, for two reasons:

1. **UX:** Don't let the user fire an admin action they're certain to be rejected for; tell them "you're not an admin" before the wire round-trip.
2. **Defensive coding:** Detect drift between our local view of 39001 and reality (rare, but possible if 39001 just changed and our cache hasn't caught up).

The validation is structural:

- For 9000/9001/9002/9005/9009: signer must be in the latest 39001 for `group_id`. If not, `ActionRejection::NotAdmin`.
- For 9007: no admin check (anyone can create a group; the founder becomes the first admin).
- For 9008: signer must be in the latest 39001. Additionally, by relay29 convention, only the *founder* (the original 9007 signer) can delete; we encode this as a soft warning in the UI but defer the hard check to the relay (since not all NIP-29 implementations enforce it).

The latest-39001 lookup is a synchronous read against `nmp-nip29::GroupAdmins`'s DomainRecord for `group_id`. If no record exists locally (cold), we treat the check as `Unknown` and **proceed with the publish anyway** — the relay is the final arbiter and a cold-cache should not block legitimate admin actions on first launch.

## 4. The relay-signed metadata trust check (open ADR territory)

The 39000–39003 events are signed by the relay's keypair. NMP must answer: which pubkey is *the* relay's keypair?

### 4.1 Three viable answers

**A. NIP-11-driven trust.** Read the relay's NIP-11 document at HTTP fetch time, look for a declared `pubkey` field, store it, accept any 39000–39003 from that pubkey thereafter. Reject events claiming to be 39000–39003 but signed by a different pubkey.

**B. First-write-wins trust (TOFU).** The first 39000 we ingest for a given `(host_relay_url, group_id)` records the signer pubkey; subsequent 39000s from a *different* pubkey are rejected with a typed `MetadataSignerChanged` error, until the user explicitly accepts a rotation.

**C. Best-effort trust.** Accept any 39000–39003 received over the wire from `host_relay_url`. The relay couldn't deliver an event signed by a wrong pubkey because we asked for events from that relay — anyone forging would have to compromise the connection itself, which is the WebSocket/TLS layer's job to prevent.

### 4.2 The trade-off

A is the strongest, requires NIP-11 to be reliable + relays to set their `pubkey` field correctly (many don't). B is robust to NIP-11 absence but introduces a user-facing prompt on key rotation. C is the easiest, doesn't require NIP-11, but a malicious relay can lie about who signed if our wire-level checks miss something subtle.

### 4.3 The M11.5 default

**Ship B (TOFU) by default, with A (NIP-11 strict) auto-promotion when NIP-11 declares a pubkey.** Earlier drafts of this doc and `kinds.md` §2.4 leaned C; reconsidered after codex flagged the spoofing risk:

> Any host relay that also accepts ordinary parameterized events would forward a user-signed kind:39001 carrying the room's `d` tag if it accepts the write. Since `GroupAdmins`/`GroupMembers` are derived *only* from these snapshots, accepting any signer-from-host-relay lets a malicious user spoof admin/membership state simply by signing and pushing a kind:39001. TLS authenticates the connection, not `event.pubkey`.

Policy B (TOFU) defeats the spoof: the first **kind:39000** (group identity) we see for `(host_relay_url, group_id)` records `(group_id, signer_pubkey)` as the cached signer for that group. **39001/39002/39003 cannot establish the pin** (the rule below in step 3); they're held in a quarantine buffer until 39000 lands, then replayed against the now-known signer. Subsequent metadata events for the same group from a *different* signer are rejected with a typed `MetadataSignerChanged` error until the user explicitly accepts a rotation. Policy A (NIP-11 strict) is even tighter — accept only metadata signed by the relay's declared NIP-11 `pubkey` — and is the auto-upgrade path when NIP-11 declares a pubkey.

The ingest hook for 39000–39003 enforces:

1. If NIP-11 declares a `pubkey` for the relay: require `event.pubkey == nip11.pubkey` (policy A).
2. Else, if `(group_id, signer)` is in the TOFU cache: require `event.pubkey == cached_signer` (policy B steady state).
3. Else (cold TOFU): **only a kind:39000 (group identity) may establish the initial pin**, never a 39001/39002/39003. The reasoning: on relays that also accept ordinary parameterized writes, a malicious user could push a kind:39001 *before* the legitimate relay-signed 39000 arrives, and that forged signer would become the cached pin. Pinning only from 39000 closes the window — even a malicious user-signed 39000 is detectable because the legitimate relay-signed 39000 (which arrives via the same relay's normal metadata stream) will conflict on `event.pubkey` and trigger `MetadataSignerChanged` immediately on the next session. The 39000 trade-off: until a 39000 has arrived, ingested 39001/39002/39003 events are **held in a quarantine buffer**, not applied to canonical `GroupAdmins`/`GroupMembers`. When the first 39000 arrives, the buffer is replayed against the now-known signer; events from the wrong signer get rejected; events from the right signer get applied in order. The buffer is bounded (default 64 events per group) and TTL-evicted after 1 hour (a relay that doesn't produce a 39000 within an hour of producing membership snapshots is misbehaving; rendering empty is the safer default).
4. On any signer mismatch under (1) or (2): reject the event, surface `MetadataSignerChanged` to the UI's diagnostics lane, leave canonical state unchanged.

The TOFU cache is per-`(host_relay_url, group_id)` (NOT per-host), persisted via M3 LMDB, and survives session restarts. A rotation prompt UX is post-M11.5; for M11.5 the typed error is sufficient (developer-facing on the diagnostics surface; user-facing as a passive "group metadata signer changed" toast).

The `nip29_metadata_signer_trust_*` test in `moderation.md` §7 asserts: **(a)** policy A accepts and policy B initial-pins when NIP-11 pubkey is absent, **(b)** subsequent events with the same signer are accepted, **(c)** events with a different signer are rejected with `MetadataSignerChanged`, **(d)** the canonical `GroupAdmins`/`GroupMembers` are *not* mutated by a rejected event.

Bootstrap-host discovery (`routing.md` §4.3) already requires policy A's NIP-11+39000 signer match before caching a host candidate — the moderation policy here governs subsequent ingest of already-pinned hosts.

## 5. The moderation audit trail (audit-only; does not mutate canonical membership)

**Canonical membership state is *only* derived from the relay-signed 39001/39002 snapshots**, never from user-signed 9000/9001 events. The relay is the trust anchor (§1); its 39001/39002 are the authoritative state; user-signed moderation events are *requests* to mutate that state, evaluated by the relay before being reflected back in an updated 39001/39002. A client that applied 9000/9001 side-effects directly to `GroupAdmins`/`GroupMembers` would be trusting user signatures over relay signatures, inverting the NIP's trust model.

Every 9000–9009 + 9021 + 9022 ingested is preserved in `nmp-nip29::GroupModerationEvent` for the lifetime of the group's records, plus 30 days for tombstoned groups (the M3 LMDB retention policy applies). **This audit record is the *only* persistent effect** of ingesting a user-signed moderation event — the `GroupAdmins`/`GroupMembers` DomainRecords stay strictly downstream of the relay's 39001/39002.

This separation also handles the race where a 9000 arrives via subscription stream before the corresponding 39002 update: the audit record materialises immediately; the canonical membership flips when (and only when) the relay's republished 39002 arrives. If the relay rejects the 9000 (e.g. signer not actually an admin), no 39002 update follows and membership stays unchanged — exactly the desired behavior.

The audit trail serves three audiences:

- **Admins reviewing past actions** ("who removed user X and when?")
- **Members investigating membership disputes** ("when did I get kicked? was a reason given?")
- **Developers debugging** ("the relay says I'm not in 39001 but I see a 9000 adding me — what's going on?")

The audit DomainRecord schema:

```rust
pub struct ModerationEvent {
    pub event_id: EventId,
    pub group_id: GroupId,           // (host_relay_url, local_id)
    pub kind: u16,                   // 9000–9009, 9021, 9022
    pub actor_pubkey: PublicKey,     // the signer
    pub target_pubkey: Option<PublicKey>,   // from p tag for user-targeting kinds
    pub target_event_id: Option<EventId>,   // from e tag for delete-event (9005)
    pub reason: Option<String>,
    pub created_at: u64,
    pub raw_tags: Vec<Tag>,          // full preservation for forensics
}
```

The ingest path materialises this audit record **and only this audit record** for any 9000-9022. It does **not** apply side-effects to the canonical 39001/39002 records — those flip only when the relay's own republished 39001/39002 arrives (per §5 first paragraph).

## 6. Membership-as-security-boundary in projections

A `private` group's 39000 is hidden from non-members. A `restricted` group rejects writes from non-members. A `closed` group rejects join requests without a code. These are *relay-enforced* — non-members can't even see the events because the relay won't serve them.

But what about the *client-side rendering*? If we somehow obtain a fragment of a private group's chat (e.g. from a stale cache before a membership change), do we render it?

**`nmp-nip29` projection rule (private-group gate only):** A `ViewModule` for a **private** group (39000 carries the `private` marker tag) whose latest known 39002 does **not** include the current user's pubkey projects an **empty result + a "you are not (or no longer) a member of this group" diagnostic**. The cache's stale events for that group are kept for 24 hours then evicted, in case membership is restored. This prevents the "I got kicked from a private group but still see its chat" rendering bug.

**Public groups are not gated.** A public group's metadata, members, admins, and discussions are visible to anyone — that's what `public` means in NIP-29. The room-preview flow (`feature-inventory.md` §1.1: "Preview sheet — read-only peek at name/about/picture/member count/admins before joining") requires this; gating public groups on membership would break preview.

The rule mapping per visibility:

| 39000 visibility | `GroupHome`/`GroupChat`/`GroupDiscussions`/`GroupMembers` rendering |
|---|---|
| `public` (default; absence of `private` marker) | always render whatever is cached, regardless of current-user membership |
| `private` | render only if current user is in latest 39002 (or 39001); else render empty + member-required diagnostic |

The `JoinedGroups` view is exempt either way (it needs to *show* groups whose membership state is changing). The `GroupExplorer` view filters to non-`hidden` groups but does not apply the membership gate (it's the discovery surface).

For `restricted` groups (public-readable, member-only-writable), the gate does **not** apply to read views; the gate applies only to *write* `ActionModule`s, which pre-validate that the signer is a member before publishing.

## 7. Tests this layer requires (M11.5 exit gate, moderation half)

These extend the routing tests in `routing.md` §8:

1. `nip29_admin_action_pre_validates_signer_in_39001` — dispatching `PutUser` without the signer in the local 39001 cache produces `ActionRejection::NotAdmin` before any wire activity.
2. `nip29_admin_action_proceeds_when_cache_cold` — same dispatch with an empty 39001 cache proceeds to the wire (relay arbitrates), records the round-trip in the diagnostics lane.
3. `nip29_previous_chain_attached_to_outbound_chat` — publishing a kind:9 chat with a populated recent-event cache attaches ≥ 3 `previous` tags with correct id prefixes.
4. `nip29_previous_chain_omitted_on_cold_cache_with_timeout` — publishing a kind:9 chat with an empty cache delays for the configured timeout then proceeds without `previous` tags; surfaces the cold-cache fact in diagnostics.
5. `nip29_moderation_event_audited_into_domain_record` — an ingested 9000 produces a `ModerationEvent` audit record with correct actor/target/reason fields **and does not mutate `GroupAdmins`/`GroupMembers`**.
6. `nip29_canonical_membership_updates_only_from_relay_snapshot` — an ingested 9000 *without* a follow-up 39002 leaves `GroupMembers` unchanged; the same scenario with the relay's reflected 39002 arriving causes `GroupMembers` to flip exactly once. The companion to test 5.
7. `nip29_private_group_projection_empties_on_membership_loss` — a `GroupChat` projection that previously rendered content empties + raises the diagnostic when the latest 39002 no longer contains the current user.
8. `nip29_metadata_signer_trust_pins_and_rejects_rotation` — the A/B trust model: a 39000 carrying a fresh signer pubkey is accepted on first sight (TOFU initial pin); subsequent 39000-39003 for the same `(host_relay_url, group_id)` with a *different* signer is rejected with `MetadataSignerChanged` and does not mutate `GroupAdmins`/`GroupMembers`. When NIP-11 declares a relay pubkey, the test variant `nip29_metadata_signer_strict_nip11_match` asserts the strict-A policy (only the NIP-11-declared pubkey accepted, no TOFU even on cold cache).

These eight complete the M11.5 exit-gate audit for the moderation half of `nmp-nip29`.

## 8. What's intentionally NOT in this design

- **No "moderator queue" UI for held join requests.** Relays that hold 9021s for review surface them via their own admin interface (web admin, console, etc.); a NIP for client-side moderator queues doesn't exist yet. If one emerges, it becomes a follow-up milestone.
- **No reputation / strikes / temporary mute.** NIP-29 doesn't define these; bolting them on would require client-side conventions that no two NIP-29 clients share. Out of scope.
- **No federation / group-bridging.** A group only exists on one relay at a time. Migration (NIP-29 mentions it) is in the open-questions list but not the design.
- **No end-to-end encryption.** Group chat is plaintext to the host relay by design. Encrypted-group is a different NIP (NIP-104 ideation work), out of scope.

Keeping the M11.5 scope honest is the doctrine: ship the NIP-29 spec faithfully, do not invent client-side moderation primitives the protocol doesn't support.
