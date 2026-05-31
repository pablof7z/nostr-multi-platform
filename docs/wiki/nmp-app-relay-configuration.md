---
title: NMP App Relay Configuration
slug: nmp-app-relay-configuration
summary: App relays are the kernel's connectivity layer — declared in Rust, persisted locally, and completely separate from the user's NIP-65 relay list. They are never published as kind:10002.
tags:
  - guide
volatility: warm
confidence: high
created: 2026-05-31
updated: 2026-06-01
verified: 2026-06-01
compiled-from: conversation
sources:
  - session:ec0e64f8-3ef7-4983-933a-f5a3e672998a
---

# NMP App Relay Configuration

## What Are App Relays?

App relays are the relays an NMP-based app uses for connectivity. They are **not** the user's NIP-65 relay list and are **never** published as a kind:10002 event. The distinction:

- **App relays** (`configured_relays`, type `AppRelay`): declared by the app author in Rust, editable by the user via settings UI, persisted locally in a JSON sidecar file. These control which relays the kernel dials.
- **User NIP-65 relays** (kind:10002): the user's declared inbox/outbox, published to the Nostr network, visible to other clients. These are routing hints used by the planner — they are NOT the same as the app's connectivity config.

NIP-65 kind:10002 is auto-published as a side effect when the user edits read/write relays in the settings UI. This is an interop convenience for other Nostr clients. The canonical source of truth is always the local `configured_relays` state.

## Builder API

```rust
use nmp_app_template::{NmpAppBuilder, RunConfig};

let app = NmpAppBuilder::new()
    .with_relay("wss://relay.primal.net", "both,indexer")
    .with_relay("wss://purplepag.es",     "indexer")
    .storage_path("/path/to/app/data")
    .start(RunConfig::default());
```

Calling `.with_relay()` at least once replaces the template's built-in defaults entirely. If no calls are made the built-in defaults (`relay.primal.net` both+indexer, `purplepag.es` indexer) are used.

## Relay Roles

| Token | Meaning |
|---|---|
| `"read"` | Fetch content from this relay |
| `"write"` | Publish content to this relay |
| `"both"` | Read + write |
| `"indexer"` | Discovery queries (kind:0/3/10002) |

Roles are additive. `"both,indexer"` means the relay serves all three purposes simultaneously.

## Persistence

On first start, `NmpAppBuilder::start()` writes the declared defaults to `{storage_dir}/.nmp-relay-config.json`. On subsequent starts it reads that sidecar instead, so user edits survive restarts. In-memory mode always uses the declared defaults (no sidecar).

## Terminology and Naming

The kernel's internal type for a configured relay is `AppRelay`; the field name is `configured_relays`; the JSON projection key is `"configured_relays"`. UI-framed names (`relay_edit_rows`, `RelayEditRow`) are banned from nmp-core. nmp-core contains zero hardcoded relay URLs in production code.

## See Also

- [14a — App relay configuration (builder guide)](../builder-guide/14a-app-relay-configuration.md)
- [nmp-relay-settings-view](nmp-relay-settings-view.md)
- [nmp-indexer-app-relay-sources](nmp-indexer-app-relay-sources.md)
