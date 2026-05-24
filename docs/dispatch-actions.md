# `dispatch_action` Namespace Catalog

> **Why this file exists:** Without it, a developer building on the framework cannot find
> what to call, what JSON shape to pass, or which snapshot projection will carry the result.
> V-35 (Opus direction review #16, 2026-05-24).
>
> **How to call an action from Swift:**
> ```swift
> let json = """{"content":"hello","reply_to_id":null,"target":"Auto"}"""
> nmp_app_dispatch_action(app, "nmp.publish", "PublishNote", json, correlationId)
> ```
> The third argument is the enum variant name for tagged enums; pass `""` for flat structs.
>
> **Projection subscriptions:** use `nmp_app_register_snapshot_projection` with the
> projection namespace as the key. The snapshot callback fires with the projection's JSON
> snapshot whenever kernel state changes.

---

## Actions (write path)

### `nmp.publish` · Core publish seam

Crate: `nmp-core/src/publish/action.rs` · Type: tagged enum `PublishAction`

#### Variant: `PublishNote` — sign and publish a kind:1 note

```json
{
  "PublishNote": {
    "content": "Hello Nostr",
    "reply_to_id": null,
    "target": "Auto"
  }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `content` | string | Note body. |
| `reply_to_id` | string? | Hex event id of the parent note. `null` for root. |
| `target` | `"Auto"` \| `{"Explicit":{"relays":["wss://…"]}}` | `"Auto"` = NIP-65 outbox routing (default, D3). |

#### Variant: `PublishProfile` — sign and publish a kind:0 profile

```json
{
  "PublishProfile": {
    "fields": { "name": "Alice", "about": "...", "picture": "https://…" }
  }
}
```

`fields` is any flat JSON object with string values. The actor serializes it into the kind:0
`content` field and signs with the active signer.

#### Variant: `Publish` — publish a pre-signed event

```json
{
  "Publish": {
    "handle": "my-handle-string",
    "event": { "id": "…", "pubkey": "…", "sig": "…", "kind": 1, "tags": [], "content": "…", "created_at": 0 },
    "target": "Auto"
  }
}
```

For callers that construct and sign events externally. `handle` is any opaque string the host
uses to correlate the result in the publish-status projection.

#### Variant: `PublishRaw` — sign and publish an arbitrary event kind

```json
{
  "PublishRaw": {
    "kind": 30023,
    "tags": [["d", "my-article"], ["title", "Hello"]],
    "content": "Article body",
    "target": "Auto"
  }
}
```

Generic path for custom event kinds. kind:0 and kind:3 are rejected here — use
`PublishProfile` or `nmp.follow` / `nmp.unfollow` respectively.

**Result projection:** `nmp.publish.status` — carries per-handle `PublishOutcome` entries
(Pending / Relayed / FailedAfterRetries).

---

### `nmp.follow` · Follow a pubkey (kind:3 update)

Crate: `nmp-nip02/src/lib.rs`

```json
{ "pubkey": "abcdef0123456789…" }
```

| Field | Type | Description |
|-------|------|-------------|
| `pubkey` | string | Target pubkey, lowercase hex. |

Appends to the active account's kind:3 follow set and re-publishes it.

---

### `nmp.unfollow` · Unfollow a pubkey (kind:3 update)

Crate: `nmp-nip02/src/lib.rs`

```json
{ "pubkey": "abcdef0123456789…" }
```

Same shape as `nmp.follow`. Removes the pubkey from kind:3 and re-publishes.

---

### `nmp.nip25.react` · React to an event (kind:7)

Crate: `nmp-nip02/src/lib.rs`

```json
{ "target_event_id": "abcdef…", "reaction": "+" }
```

| Field | Type | Description |
|-------|------|-------------|
| `target_event_id` | string | Hex event id to react to. |
| `reaction` | string | NIP-25 reaction content. `"+"` (like) is the default; `"-"` (dislike) or any emoji shortcode. |

`reaction` may be omitted — defaults to `"+"`.

---

### `nmp.nip17.send` · Send a NIP-17 encrypted DM (kind:14 rumor)

Crate: `nmp-nip17/src/action.rs`

```json
{
  "recipient_pubkey": "abcdef…",
  "content": "Hey!",
  "reply_to": null
}
```

| Field | Type | Description |
|-------|------|-------------|
| `recipient_pubkey` | string | Recipient's pubkey, lowercase hex. |
| `content` | string | Plaintext message body (kernel seals + gift-wraps). |
| `reply_to` | string? | Hex event id of the parent message. `null` for new thread. |

---

### `nmp.nip17.publish_relay_list` · Publish DM-inbox relay list (kind:10050)

Crate: `nmp-nip17/src/dm_relay_list.rs`

```json
{ "relays": ["wss://relay.example.com", "wss://inbox.example.com"] }
```

Publishes a kind:10050 NIP-17 DM-inbox relay list. Rejects empty `relays` (would clear
the DM-inbox cache entry — use an explicit clear verb if intentional).

---

### `nmp.nip57.zap` · Send a NIP-57 zap (kind:9734)

Crate: `nmp-nip57/src/action.rs`

```json
{
  "recipient_pubkey": "abcdef…",
  "amount_msats": 1000,
  "lnurl": "lnurl1…",
  "relays": [],
  "target_event_id": null,
  "comment": null
}
```

| Field | Type | Description |
|-------|------|-------------|
| `recipient_pubkey` | string | Recipient's pubkey, lowercase hex. Required. |
| `amount_msats` | u64 | Amount in millisatoshis. Must be > 0. |
| `lnurl` | string | LNURL-pay endpoint (lightning address, bech32 LNURL, or bare `https://`). Required. |
| `relays` | string[] | Relay URLs for the kind:9734 `relays` tag. Empty = auto-select from NIP-65. |
| `target_event_id` | string? | Hex event id of the zapped note (`null` for profile zaps). |
| `comment` | string? | Optional free-form comment — becomes kind:9734 `content`. |

---

### `nmp.wallet.pay_invoice` · Pay a BOLT-11 invoice via NIP-47 wallet

Crate: `nmp-core/src/wallet/action.rs` · Type: tagged enum `WalletAction`

```json
{
  "PayInvoice": {
    "bolt11": "lnbc…",
    "amount_msats": null
  }
}
```

| Field | Type | Description |
|-------|------|-------------|
| `bolt11` | string | BOLT-11 invoice string. |
| `amount_msats` | u64? | Override for zero-amount invoices. `null` = use invoice's embedded amount. |

---

### `nmp.nip65.publish_relay_list` · Publish NIP-65 relay list (kind:10002)

Crate: `nmp-nip65/src/lib.rs`

```json
{
  "relays": [
    { "url": "wss://relay.example.com", "marker": "Both" },
    { "url": "wss://read-only.example.com", "marker": "Read" },
    { "url": "wss://write-only.example.com", "marker": "Write" }
  ]
}
```

`marker` defaults to `"Both"` when absent. Rejects empty `relays` (would clear the NIP-65
cache entry — destructive). URLs that do not parse as `ws://` or `wss://` are dropped.

---

### `nmp.nip29.join` · Join a NIP-29 group (kind:9021)

Crate: `nmp-nip29/src/action/join.rs`

```json
{
  "group": { "host_relay_url": "wss://groups.example.com", "local_id": "my-group" },
  "invite_code": null,
  "reason": null
}
```

| Field | Type | Description |
|-------|------|-------------|
| `group` | GroupId | `{host_relay_url, local_id}` identifying the group. |
| `invite_code` | string? | Optional invite code for closed groups. |
| `reason` | string? | Optional join-request message content. |

---

### `nmp.nip29.discover` · Subscribe to a relay's group catalog

Crate: `nmp-nip29/src/action/discover.rs`

```json
{ "relay_url": "wss://groups.example.com" }
```

Opens a subscription for NIP-29 group metadata (kind:39000) on the given relay. The
discovered groups appear in the group-list snapshot projection.

---

### `nmp.nip29.post_chat_message` · Post a kind:9 group chat message

Crate: `nmp-nip29/src/action/content.rs`

```json
{
  "group": { "host_relay_url": "wss://groups.example.com", "local_id": "my-group" },
  "content": "Hello group!",
  "previous_event_id_prefixes": [],
  "reply_to_event_id": null
}
```

| Field | Type | Description |
|-------|------|-------------|
| `group` | GroupId | Target group identity. Must be routable (host_relay_url must be non-empty). |
| `content` | string | Message body. |
| `previous_event_id_prefixes` | string[] | NIP-29 causal ordering — pull from `RecentGroupEvents::previous_tags_for`. Pass `[]` if unavailable. |
| `reply_to_event_id` | string? | Hex event id of the parent message in thread. |

---

### `nmp.nip29.react_in_group` · React to a group event (kind:7 + `h` tag)

Crate: `nmp-nip29/src/action/composed.rs`

```json
{
  "group": { "host_relay_url": "wss://groups.example.com", "local_id": "my-group" },
  "target_event_id": "abcdef…",
  "target_author_pubkey": null,
  "content": "+"
}
```

In-group reaction — includes `h` tag so the reaction stays scoped to the group relay.

---

## Projections (read path)

Register with `nmp_app_register_snapshot_projection(app, namespace, callback)`. The callback
fires with a JSON snapshot string whenever kernel state changes.

| Namespace | Crate | Description |
|-----------|-------|-------------|
| `nmp.publish.status` | `nmp-core/src/publish/view.rs` | Per-handle publish outcomes. Map from handle → `Pending \| Relayed \| FailedAfterRetries`. |
| `nmp.nip57.zaps` | `nmp-nip57/src/view.rs` | Zap aggregate per event id — total msats, receipt count. |
| `nmp.nip01.replies` | `nmp-nip01/src/view.rs` | Reply thread for a single event id. |
| `nmp.nip01.thread` | `nmp-nip01/src/view.rs` | Full thread rooted at a given event id. |
| `nmp.nip01.modular_timeline` | `nmp-nip01/src/meta_timeline.rs` | Modular timeline — ordered `TimelineItem` list for the home feed. |

---

## Action registration (Rust side)

Each `ActionModule` is registered at app-init via `ActionRegistry::register`:

```rust
// in your app's init function:
nmp_nip17::register_actions(app);   // registers nmp.nip17.send + nmp.nip17.publish_relay_list
nmp_nip57::register_actions(app);   // registers nmp.nip57.zap
nmp_nip02::register_actions(app);   // registers nmp.follow, nmp.unfollow, nmp.nip25.react
nmp_nip65::register_actions(app);   // registers nmp.nip65.publish_relay_list
nmp_nip29::register_actions(app);   // registers nmp.nip29.*
// nmp.publish + nmp.wallet.pay_invoice are registered automatically by nmp_app_chirp_register
```

The canonical example is `crates/nmp-app-chirp/src/ffi/register.rs`.
