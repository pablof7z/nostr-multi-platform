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

Use `NmpAppBuilder::with_relay(url, role)` to declare the app's default relay
set. The builder carries these as defaults; on first start they are written to
the `.nmp-relay-config.json` sidecar alongside the LMDB store.

```rust
use nmp_defaults::{NmpAppBuilder, RunConfig};

let app = NmpAppBuilder::new()
    .with_relay("wss://relay.primal.net", "both,indexer")  // read + write + discovery
    .with_relay("wss://purplepag.es",     "indexer")       // discovery only
    .storage_path("/path/to/app/data")
    .start(RunConfig::default());
```

On every **subsequent** start the builder reads the persisted sidecar instead
of the declared defaults, so user edits (add/remove relay) survive app
restarts.

### In-memory apps (tests, CLI tools)

```rust
let app = NmpAppBuilder::new()
    .with_relay("wss://nos.lol", "both")
    .in_memory()
    .start(RunConfig::default());
```

In-memory mode always uses the declared defaults; there is no sidecar.

### Using built-in defaults

Calling `.with_relay()` at least once **replaces** the built-in defaults
entirely. If you make no `.with_relay()` calls the builder uses the
nmp-defaults defaults (`relay.primal.net` both+indexer, `purplepag.es`
indexer). Most apps should declare explicit relays rather than relying on
defaults.

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
.with_relay("wss://relay.primal.net", "both,indexer")

// Read-only content relay
.with_relay("wss://nos.lol", "read")

// Write-only relay (outbox)
.with_relay("wss://relay.damus.io", "write")
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
  { "url": "wss://relay.primal.net", "role": "both,indexer" },
  { "url": "wss://purplepag.es",     "role": "indexer" }
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

**4. Calling with_relay after storage is set**

```rust
// DOES NOT COMPILE — with_relay is available on both builder states, but
// calling it after start() is impossible (start() consumes the builder).
let app = NmpAppBuilder::new()
    .storage_path("/data")
    .start(RunConfig::default());
app.with_relay(...)   // ← compile error: NmpAppBuilder is gone
```

All relay declarations must be made before `start()`. Post-start relay changes
go through the dispatch API (`ActorCommand::AddRelay`).

---

## Quick reference

```rust
// Declare defaults (Rust composition root)
NmpAppBuilder::new()
    .with_relay(url, role)            // accumulates; first call replaces built-ins
    .with_relays(vec![(url, role)])   // bulk variant
    .storage_path(dir)
    .start(config)

// Read configured relays from another Rust crate (e.g. a runtime controller)
let slot: AppRelaySlot = app.configured_relays_handle();
let guard = slot.lock().unwrap();
for relay in guard.as_slice() {
    println!("{} — {}", relay.url(), relay.role());
}

// Add/remove at runtime (via action dispatch, not direct mutation — D4)
app.dispatch_action("nmp.relay.add",   json!({ "url": "wss://…", "role": "both" }));
app.dispatch_action("nmp.relay.remove", json!({ "url": "wss://…" }));
```

---

See also:
- [14 — Subscription lifecycle + relay manager + NIP-42](14-relay-manager.md)
- [07 — Subscription planner — Interest → CompiledPlan → wire](07-subscription-planner.md)
- [05a — Substrate traits](05a-substrate-traits.md)
- [10 — Outbox routing](10-outbox-routing.md)
