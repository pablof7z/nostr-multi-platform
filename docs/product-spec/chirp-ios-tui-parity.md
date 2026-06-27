# Chirp iOS / TUI Feature Parity

This inventory maps every user-facing Chirp iOS feature to the terminal surface.
Both shells stay thin: Rust owns protocol state, projections, and action policy;
each shell formats raw projection fields for its presentation surface and
dispatches the same shared FFI and typed action envelopes.

| iOS area | iOS feature | TUI surface |
| --- | --- | --- |
| Onboarding | Create new identity | `Settings` tab, `:account create <name> [relay...]` |
| Onboarding | Import `nsec` | `:account import <nsec>` |
| Onboarding | Import local key with Marmot MLS | `:account import-mls <nsec>` |
| Onboarding | NIP-46 bunker sign-in | `:account bunker <uri>` |
| Onboarding | Nostr Connect QR/deep-link URI | `:account nostrconnect` |
| Onboarding | Cancel bunker handshake | `:account cancel-bunker` |
| Home | Shared timeline | `Home` tab |
| Home | Compose note | `i`, then `Ctrl+Enter` |
| Home | Reply to selected note | `r`, then `Ctrl+Enter` |
| Home | React to selected note | `+` |
| Home | Follow/unfollow selected author | `f` / `F` |
| Navigation | Open thread | `Enter`, or `:search thread <event-id>` |
| Navigation | Open profile | `p`, or `:search profile <pubkey>` |
| Navigation | Open firehose tag search | `:search tag <tag>` |
| Profile | Render kind:0 profile metadata | Profile pane from kernel-resolved profile metadata |
| Profile | Publish kind:0 profile metadata | `:profile set name=<n> about=<text> picture=<url> nip05=<id>` |
| Chats | NIP-17 inbox projection | `Chats` tab |
| Chats | Send direct message | `:dm <pubkey> <message>` |
| Chats | Publish DM relay list | `:dm-relays <relay> [relay...]` |
| Groups | Discover NIP-29 groups | `Groups` tab, `:group discover <relay>` |
| Groups | Create NIP-29 or Marmot MLS group | `Groups` tab, `n` opens the centered Create group modal |
| Groups | Join NIP-29 group | `:group join <relay> <local-id>` |
| Groups | Open NIP-29 group chat projection | `:group open <relay> <local-id>` |
| Groups | Post NIP-29 chat message | `:group post <relay> <local-id> <message>` |
| Groups | React/reply in NIP-29 group | `:group react ...`, `:group reply ...` |
| Groups | Marmot MLS active registration | `:mls init` |
| Groups | Marmot MLS snapshot/actions | `:mls snapshot`, `:mls dispatch <json>` |
| Wallet | NWC status/balance | `Wallet` tab |
| Wallet | Connect wallet | `:wallet connect <nostr+walletconnect-uri>` |
| Wallet | Pay invoice | `:wallet pay <bolt11> [amount_msats]` |
| Wallet | Disconnect wallet | `:wallet disconnect` |
| Settings | Account list, active account | `Settings` tab |
| Settings | Switch/remove account | `:account switch <id>`, `:account remove <id>` |
| Settings | Active relay inventory + relay list/editor | `Settings` tab, `:relay add/remove` |
| Settings | Publish outbox and settled history detail | `Settings` tab, `Enter` opens active or Published rows |
| Settings | Retry/cancel/clear publish handle | `r` / `d` in outbox detail, `:outbox retry <handle>`, `:outbox cancel <handle>` |
| Settings | Relay diagnostics/interests | `Settings` tab diagnostics and status bar |

Relay diagnostics rows render Rust-owned fields only: runtime role/category,
configured app-relay role when present, active subscription count, durable
session event count, and status/error text. Settings must show every active
relay, grouped by category/source, and selecting a relay must expose why the
client is connected, raw REQ filter JSON for each wire subscription, per-sub
event counts, EOSE/close state, reconnect/traffic counters, and the same
session event count shown in the relay preview. A zero event count must be
explainable as no active REQ, active REQ with no matches, EOSE/no matches, or a
routing/configuration anomaly. Configured indexer relays must visibly
participate in discovery-kind routing (`0`, `3`, `10002`, and other
`10000..19999` lists) or expose why they did not.

## Notes

- Tab keys mirror iOS top-level navigation: `h` Home, `c` Chats, `g` Groups,
  `w` Wallet, `s` Settings; `Tab` and `BackTab` cycle.
- The TUI uses modal forms for forms-heavy flows entered from task surfaces,
  while command mode remains available as the power-user path into shared Rust
  capability actions.
- Author/thread detail panes render dynamic feed sidecars
  (`nmp.feed.author.<pubkey>` / `nmp.feed.thread.<event-id>`). Single-pane
  shells keep at most the home feed plus one currently viewed dynamic feed open;
  opening a new dynamic pane unregisters the previous dynamic feed, and leaving
  the pane unregisters it.
- Author kind:0 rendering and note relation counts follow the render-intent
  model: visible note authors resolve through the unified profile ref path,
  profile headers prefer the kernel-owned `refs.profile` row, and names update
  when the keyed profile projection emits newer metadata.
