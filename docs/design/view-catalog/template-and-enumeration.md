# View Catalog: Template And Enumeration

[Back to Design: View Catalog](../view-catalog.md)

# Design: View Catalog

> **Audience:** Framework contributors building reference Nostr protocol modules. Each view kind below is a `ViewModule` shipped in a reusable protocol crate such as `nmp-nip01`, `nmp-nip10`, `nmp-nip25`, `nmp-nip65`, or `nmp-nip17`.

> **Status:** Rev 2, reframed per ADR-0009. These view kinds are not in `nmp-core`; apps consume them by adding the owning module crate to `nmp.toml` and regenerating the per-app FFI crate.

> **Prerequisites:** `product-spec.md` §7.6, `reactivity.md`, `kernel-substrate.md` §3, ADR-0005, ADR-0010.

---

## 1. Per-view-kind template

Every reference Nostr view module lives in a `nmp-nip*` crate and implements `ViewModule` from `nmp-core::substrate`:

```
crates/nmp-<protocol>/src/views/<kind>.rs
```

with this public shape:

```rust
pub struct <Kind>Module;

impl ViewModule for <Kind>Module {
    const NAMESPACE: &'static str = "nipXX.<kind>";

    type Spec = <Kind>Spec;
    type Payload = <Kind>View;
    type Delta = <Kind>Delta;
    type Key = <Kind>Key;
    type State = <Kind>State;

    fn key(spec: &Self::Spec) -> Self::Key;
    fn dependencies(spec: &Self::Spec) -> ViewDependencies;
    fn open(ctx: &ViewContext, spec: Self::Spec) -> (Self::State, Self::Payload);
    fn on_event_inserted(ctx: &ViewContext, state: &mut Self::State, event: &KernelEvent)
        -> Option<Self::Delta>;
    fn on_event_removed(ctx: &ViewContext, state: &mut Self::State, id: &EventId)
        -> Option<Self::Delta>;
    fn on_event_replaced(
        ctx: &ViewContext,
        state: &mut Self::State,
        old_id: &EventId,
        new_event: &KernelEvent,
    ) -> Option<Self::Delta>;
    fn on_projection_changed(ctx: &ViewContext, state: &mut Self::State, change: &ProjectionChange)
        -> Option<Self::Delta>;
    fn snapshot(ctx: &ViewContext, state: &Self::State) -> Self::Payload;
}
```

For each kind below, the catalog documents spec, payload, delta variants, dependencies, recompute strategy, pagination, best-effort placeholders, and subtleties learned from Applesauce/NDK-style clients.

---

## 1.1 Platform cache key

The generated platform wrapper organizes the shadow as typed domain-keyed dictionaries, not as a flat `[ViewId: ViewPayload]` map.

| View kind | Platform cache key | Wrapper API |
|---|---|---|
| Profile | pubkey | `useProfile(pubkey)` / `@Profile` |
| Contacts | pubkey | `useContacts(pubkey)` |
| Mailboxes | pubkey | `useMailboxes(pubkey)` |
| Mutes | active account pubkey | `useMutes()` |
| Blossom servers | pubkey | `useBlossomServers(pubkey)` |
| Timeline | spec hash | `useTimeline(spec)` |
| Thread | root event id | `useThread(rootEventId)` |
| Replies | target event id | `useReplies(targetEventId)` |
| Reactions | target event coord | `useReactions(target)` |
| Conversation list | active account pubkey | `useConversationList()` |
| Conversation | peer pubkey or group id | `useConversation(peer)` |
| Zap history | active account pubkey | `useZapHistory()` |
| Wallet balance | wallet id | `useWallet()` |
| WoT rank | pubkey | `useWotRank(pubkey)` |
| Search | spec hash | `useSearch(query)` |

`ViewId` is an internal FFI token. Wrappers refcount per key, dispatch `OpenView`/`CloseView` to Rust, and enforce the same warmth/eviction policy as the kernel.

## 2. View kinds

| # | Kind | Protocol module | Detailed in this doc? | Phase |
|---|---|---|---|---|
| 1 | Profile | `nmp-nip01` | yes | 1a.2 |
| 2 | Contacts | `nmp-nip02` | stub | 1a.4 |
| 3 | Mailboxes | `nmp-nip65` | stub | 1a.4 |
| 4 | Mutes | `nmp-nip01` | stub | post-1a |
| 5 | Blossom servers | `nmp-blossom` | stub | post-1a |
| 6 | Timeline | `nmp-nip01` | yes | 1a.4 |
| 7 | Thread | `nmp-nip10` | yes | 1a.6 |
| 8 | Replies | `nmp-nip10` | stub | 1a.6 |
| 9 | Reactions | `nmp-nip25` | yes | 1a.6 |
| 10 | Conversation list | `nmp-nip17` | stub | post-1a |
| 11 | Conversation | `nmp-nip17` | yes | post-1a |
| 12 | Zap history | `nmp-nip57` | stub | post-1a |
| 13 | Wallet balance | `nmp-nwc` or `nmp-nip60` | stub | post-1a |
| 14 | WoT rank | `nmp-wot` | stub | post-1a |
| 15 | Search | TBD utility module | stub | post-1a |
