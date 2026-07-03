# 14a — App relay configuration

> Status: **SHIPS** · Audience: **agents + app authors** · Doctrine: **D0**
> (no app/protocol nouns in the substrate), D3 (outbox routing), D4 (actor is
> sole writer).

App relay configuration is how your Rust app tells the NMP kernel which relays
to use. This is **entirely separate from the user's NIP-65 relay list** — a
distinction that trips up almost every new NMP author.

---

## The two relay concepts

| | App relays | User NIP-65 relays |
|---|---|---|
| **What** | The relays your app connects to | The user's declared inbox/outbox on Nostr |
| **Declared by** | App author in Rust | User, via relay settings UI |
| **Published to Nostr?** | No | Yes — kind:10002 event |
| **Kernel type** | `AppRelay` (`configured_relays`) | Routing data in `mailboxes` cache |
| **Persisted where** | `.nmp-relay-config.json` sidecar | Fetched from the network (kind:10002) |
| **Who can change it?** | App author sets defaults; user can edit via UI | User publishes a new kind:10002 |

**App relays are the app's connectivity layer.** They determine which relays
the kernel dials for fetching and publishing. They do not touch the user's
Nostr identity — the user's NIP-65 kind:10002 is managed separately and
published only when the user explicitly edits their relay list through the UI.

NIP-65 kind:10002 *is* auto-published as a side effect when the user adds or
removes a read/write relay through the settings UI — this is an interop
convenience so other clients can discover the user's updated relay set. But the
source of truth is the app's local `configured_relays` state, not a network
round-trip.

---

## Declaring app relays in Rust

Use `NmpAppBuilder::with_relays([(url, role), ...])` to declare the app's
default relay set. The runnable builder export is
`nmp-native-runtime::NmpAppBuilder`. The builder carries relay defaults; on
first start they are written to the `.nmp-relay-config.json` sidecar
alongside the LMDB store.

```rust
use nmp_native_runtime::{NmpAppBuilder, RunConfig};

let app = NmpAppBuilder::new()
    .storage_path("/path/to/app/data")
    .consume_all_builtin_projections()
    .with_relays([
        ("wss://primary-relay.example", "both,indexer"),
        ("wss://indexer-relay.example", "indexer"),
    ])
    .start(RunConfig::default());
```

On every **subsequent** start the builder reads the persisted sidecar instead
of the declared defaults, so user edits (add/remove relay) survive app
restarts.

### In-memory apps (tests, CLI tools)

```rust
let app = NmpAppBuilder::new()
    .in_memory()
    .consume_all_builtin_projections()
    .with_relays([("wss://test-relay.example", "both")])
    .start(RunConfig::default());
```

In-memory mode always uses the declared defaults; there is no sidecar.

### No built-in defaults

NMP ships no real public relay URLs and no defaults bundle that can provide
them. Apps must make an explicit initial-relay decision before `start()`:

- `.with_relays([...])` declares the app-owned operator defaults.
- `.without_initial_relays()` starts with an empty `configured_relays` set for
  offline/test/local apps.

There is no framework fallback. If the app chooses no initial relays, network
operations fail closed until the user or host adds relays at runtime.

### Search defaults

The same app-owned rule applies to relay-like defaults outside the main app
relay set:

- NIP-50 search relays use the user's kind:10007 list as the first authority.
  If the user has no list, the app may pass an app-owned
  `nmp_nip51::SearchFallbackRelays::with_default_relays([...])` into the NIP-51
  search-relay runtime registration.
  If both are empty, search stays cache-only and does not fan out to a public
  relay chosen by NMP.
- NIP-29 group-create suggestions (e.g. a suggested relay URL to pre-fill on
  a create-group form) are ordinary app/operator policy: a leaf app owns that
  string as plain config. There is no kernel projection for it — a prior
  `nmp-nip29` `GroupDefaultsProjection` round-trip for this promoted a static
  product default into protocol snapshot/codegen machinery and was removed.

For native apps, expose this kind of product default through the leaf app's
own facade, not through `nmp-nip29`:

```rust
#[uniffi::export]
impl ChirpApp {
    pub fn suggested_public_group_relay_url(&self) -> String {
        nmp_chirp_config::chirp_public_group_relay_url().to_string()
    }
}
```

The shell may use that value to seed an editable text field. The kernel does
not observe it, replay it, encode it as a projection, or treat it as protocol
state.

---

## Relay roles

A relay's role controls which lanes it participates in:

| Role token | Meaning |
|---|---|
| `"read"` | Fetch content (kind:1, kind:0, etc.) from this relay |
| `"write"` | Publish content to this relay |
| `"both"` | Read + write (equivalent to `"read write"`) |
| `"indexer"` | Discovery queries — kind:0, kind:3, kind:10002 |

Roles are **additive** — a single relay can serve multiple roles. Combine them
with a comma or space:

```rust
// Both content and indexer on the same relay
.with_relays([("wss://primary-relay.example", "both,indexer")])

// Read-only content relay
.with_relays([("wss://read-relay.example", "read")])

// Write-only relay (outbox)
.with_relays([("wss://write-relay.example", "write")])
```

The kernel's planner uses role tags to route: indexer-role relays receive
discovery REQs (kind:0/3/10002 lookups); read-role relays receive content REQs;
write-role relays receive publishes. A relay without a matching role tag is
never targeted for that operation type.

---

## Runtime changes and persistence

The user can add, remove, or change roles of relays through the settings UI.
Each mutation goes through `ActorCommand::AddRelay` / `ActorCommand::RemoveRelay`,
which:

1. Updates the kernel's `configured_relays` immediately.
2. Spawns or shuts down the relay worker socket.
3. (When read/write relays change) auto-publishes a new NIP-65 kind:10002 for
   interop — **this is the only time app relay changes touch NIP-65**.
4. Writes the updated list back to the `.nmp-relay-config.json` sidecar so it
   survives the next restart.

There is no kernel callback you need to wire — the dispatch layer handles
all four steps.

---

## The sidecar file

`{storage_dir}/.nmp-relay-config.json` is a plain JSON array:

```json
[
  { "url": "wss://primary-relay.example", "role": "both,indexer" },
  { "url": "wss://indexer-relay.example", "role": "indexer" }
]
```

- Written by `NmpAppBuilder::start()` on first run with the builder defaults.
- Read by `NmpAppBuilder::start()` on subsequent runs.
- Updated in-place whenever the user adds or removes a relay.
- Never read by nmp-core — it is purely an app-template concern.

---

## Anti-patterns

**1. Assuming app relays == NIP-65 user relays**

```rust
// WRONG — reads configured_relays and publishes them as the user's NIP-65 list.
// configured_relays is the app's connectivity config, not the user's identity.
let rows = app.configured_relays_handle().lock().unwrap();
publish_as_nip65(rows.as_slice());  // ← do not do this
```

NIP-65 publishing is handled automatically by the dispatch layer when the user
edits read/write relays. You never need to wire it manually.

---

**2. Hardcoding relay URLs inside nmp-core or any substrate crate**

```rust
// WRONG — relay URLs belong to the app layer, never to nmp-core.
// nmp-core has zero hardcoded relay URLs.
const MY_RELAY: &str = "wss://relay.example";  // in crates/nmp-core/src/
```

All relay URLs are app-provided. If you need a relay URL in a substrate-level
crate, receive it through the `AppHost` trait, an `ActorCommand`, or
`configured_relays_handle()`.

---

**3. Using configured_relays as a subscription routing oracle**

```rust
// WRONG — routing decisions live in the planner, not in a direct read of
// configured_relays. The planner uses relay roles internally.
let rows = app.configured_relays_handle().lock().unwrap();
let target = rows.as_slice().iter().find(|r| r.role().contains("indexer"));
send_req(target.unwrap().url(), filter);  // ← bypasses planner routing
```

Push an `Interest` via `nmp_app.push_interest(...)` and let the planner choose
the relay. Direct relay targeting is reserved for explicit `PublishTarget::Explicit`
publishes and for Marmot-style protocol bridges that hold an `AppRelaySlot`.

---

**4. Calling relay declarations after start**

```rust
// DOES NOT COMPILE — start() consumes the builder.
let app = NmpAppBuilder::new()
    .storage_path("/data")
    .consume_all_builtin_projections()
    .without_initial_relays()
    .start(RunConfig::default());
app.with_relays(...)   // compile error: NmpAppBuilder is gone
```

All relay declarations must be made before `start()`. Post-start relay changes
go through the dispatch API (`ActorCommand::AddRelay`).

---

## Quick reference

```rust
// Declare defaults (Rust composition root)
NmpAppBuilder::new()
    .storage_path(dir)
    .consume_all_builtin_projections()
    .with_relays(vec![(url, role)])   // explicit app-owned initial relays
    .start(config)

NmpAppBuilder::new()
    .in_memory()
    .declare_consumed_projections(["profile"])
    .without_initial_relays()         // explicit empty relay set
    .start(config)

// Read configured relays from another Rust crate (e.g. a runtime controller)
let slot: AppRelaySlot = app.configured_relays_handle();
let guard = slot.lock().unwrap();
for relay in guard.as_slice() {
    println!("{} — {}", relay.url(), relay.role());
}

// Add/remove at runtime through typed intent helpers, not direct mutation — D4.
relay_actions.add("wss://relay.example", RelayRole::Both);
relay_actions.remove("wss://relay.example");
```

---

See also:
- [14 — Subscription lifecycle + relay manager + NIP-42](14-relay-manager.md)
- [07 — Subscription planner — Interest → CompiledPlan → wire](07-subscription-planner.md)
- [05a — Substrate traits](05a-substrate-traits.md)
- [10 — Outbox routing](10-outbox-routing.md)
