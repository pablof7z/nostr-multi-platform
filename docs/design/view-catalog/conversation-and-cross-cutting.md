# View Catalog: Conversation And Cross-Cutting Concerns

[Back to Design: View Catalog](../view-catalog.md)

## 7. View: Conversation

A paginated message list for a single NIP-17 DM peer (1:1 or a single group). Messages are decrypted server-side in the actor; plaintext crosses FFI only as fields of `ConversationMessage`.

### Spec

```rust
pub struct ConversationSpec {
    pub peer: PeerRef,                    // single pubkey for 1:1, group id for group
    pub limit: usize,                     // soft cap on messages held in payload
}

pub enum PeerRef {
    Direct(PubKey),
    Group(GroupId),
}
```

### Payload

```rust
#[derive(Clone, uniffi::Record)]
pub struct ConversationView {
    pub peer_display: ProfileView,        // for direct; placeholder for group
    pub messages: Vec<ConversationMessage>,
    pub cursor: Cursor,
    pub typing_indicators: Vec<PubKey>,   // future: NIP-XX typing
    pub unread_count: u32,
    pub last_read_ms: u64,
}

#[derive(Clone, uniffi::Record)]
pub struct ConversationMessage {
    pub id: String,
    pub author_pubkey: String,
    pub author_display: String,           // pre-formatted
    pub body: String,                     // plaintext, never crosses unencrypted
    pub created_at_ms: u64,
    pub created_at_display: String,
    pub attachments: Vec<MediaRef>,
    pub reply_to: Option<String>,
    pub reactions: ReactionSummary,
    pub delivery: DeliveryState,          // Sent | Delivered | Read | Failed
}
```

### Delta

```rust
#[derive(Clone, uniffi::Enum)]
pub enum ConversationDelta {
    Appended { messages: Vec<ConversationMessage> },
    Prepended { messages: Vec<ConversationMessage> },  // pagination
    Updated { id: String, message: ConversationMessage },
    Removed { ids: Vec<String> },
    UnreadCountChanged { count: u32 },
    DeliveryUpdated { id: String, state: DeliveryState },
}
```

### Dependencies

```rust
Dependencies {
    kinds: vec![1059],                    // NIP-59 gift wraps
    p_tag_refs: vec![active_account.pubkey],  // wraps addressed to us
    ..Default::default()
}
```

Plus peer-side: when the active account sends a message, the store also receives the gift-wrapped copy addressed to the peer and stores it locally for the conversation history.

### Recompute strategy

Incremental. On gift-wrap arrival:

1. Decrypt via NIP-44 in the actor.
2. Unwrap the inner rumor; check `p` tags include our pubkey and identify the sender.
3. Build `ConversationMessage`; insert into `messages` sorted by `created_at_ms`.
4. Emit `Appended` (if at tail) or `Updated`/structural delta (if out-of-order, rare).

Sending: the action publishes the gift wrap and atomically inserts the message into the conversation with `delivery: Sent`. Receipt of the relay's `OK` updates to `Delivered`. NIP-25-style read receipts (if we support them) update to `Read`.

### Pagination

Older messages: `AppAction::AdvanceCursor` triggers a sync against the peer's inbox relays (or the active account's inbox relays for received messages) for kind:1059 events older than the current cursor. Decrypt + insert via `Prepended`.

### Best-effort placeholders

- Peer with no kind:0 → `peer_display` is placeholder profile.
- Decryption failure → don't add to view; record in `DebugDiagnostics`. Doctrine: never expose ciphertext as a message.
- Send-failure (no inbox relay reachable) → `delivery: Failed`, message stays in view with a retry affordance.

### Subtleties

- **Decryption is expensive but synchronous to the actor.** A long batch of incoming wraps could block other actor work. Offload decryption to the tokio runtime as a `CoreMsg::DecryptedRumor`, then handle insert synchronously.
- **Sender spoofing.** The inner rumor's `pubkey` is the claimed sender. Verify against the wrap's NIP-59 sealed signature.
- **Read receipts.** NIP-17 doesn't define read receipts directly. If we implement them, decide whether they're per-platform (encryption-extension) or out-of-scope for v1. Currently out-of-scope.
- **Background decryption.** When the app is backgrounded and a push notification arrives, the NSE crate (`nmp-nse`, spec §7.14) decrypts and emits a notification. On next foreground, the conversation view rebuilds from store, picking up what the NSE already inserted. Plaintext in conversation view fields is the same in both paths.

---

## 8. Cross-cutting concerns

### 8.1 The `Projections` cache, in detail

Projections are derived from events but cached for cross-view reuse. v1 catalog:

| Projection | Source events | Updated on |
|---|---|---|
| `author_display` | kind:0 | kind:0 insert; NIP-05 verification completion |
| `author_picture` | kind:0 | same |
| `author_nip05` | kind:0 + DNS verification | kind:0 insert; NIP-05 async result |
| `reaction_summary` | kind:7 referring to target | kind:7 insert; kind:5 delete |
| `zap_total` | kind:9735 referring to target | kind:9735 insert |
| `reply_count` | kind:1 with `e`-tag to target | kind:1 insert; kind:5 delete |
| `follow_status` | kind:3 of active account | kind:3 of active account |
| `mute_status` | kind:10000 of active account | kind:10000 of active account |
| `relay_list` | kind:10002 per pubkey | kind:10002 insert |

Each projection emits a `ProjectionChange::<Kind> { key, new_value }` when it changes. Views indexed by the relevant key (`by_author`, `by_e_tag`, etc.) receive `on_projection_changed`.

### 8.2 The `Cursor` enum

Used by paginated views (Timeline, Conversation, Search, Zap history). Variants:

```rust
pub enum Cursor {
    Empty,
    AtHead,
    AtTimestamp { ts: Timestamp, event_id: String },
}
```

Cursor advancement is an action; views handle it via `on_cursor_advance` (added to the per-kind contract for paginated kinds only).

### 8.3 `EventCoord`

```rust
#[derive(Clone, uniffi::Record)]
pub struct EventCoord {
    pub id: String,
    pub author: String,
    pub kind: u16,
    pub relay_hint: Option<String>,
    pub d_tag: Option<String>,            // for parameterized replaceable
}
```

Used wherever a view refers to a specific event, especially for replaceable/parameterized-replaceable references where the (author, kind, d_tag) tuple is the actual identity.

---
