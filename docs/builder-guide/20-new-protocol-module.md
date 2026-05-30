# 20 — Adding a new protocol module (`nmp-nip29` as reference)

`nmp-nip29` is the canonical worked example: a reusable NIP-29-groups protocol
crate that adds **zero group nouns to `nmp-core`** and imports **no other
`nmp-nip*` crate**. Its boundary statement
(`crates/nmp-nip29/src/lib.rs:1-57`, esp. lines 10–32) is the contract every
new protocol module copies. This section is the recipe.

## Protocol module vs app module — decision table

| Question | Protocol crate (`nmp-nip<N>`) | App core (`<app>-core`) |
|---|---|---|
| Is it a reusable Nostr concept (groups, mailboxes, gift-wrap)? | yes | no |
| Could a *second, unrelated* app consume it unchanged? | yes (NIP-29 → Highlighter, Chachi, 0xchat) | no (only Podcast cares about `Episode`) |
| Does it encode app-specific *policy* (ranking, UX rules)? | no — mechanism only | yes |
| Does it own app domain records? | no — protocol nouns only | yes |
| Does it import another `nmp-nip*`? | **never** — compose at app layer | may depend on several |

Heuristic: a protocol crate is a **reusable mechanism**; an app core is
**policy + app nouns**. "I might reuse this someday" does **not** justify a
protocol crate — concrete cross-app reuse does. The four-layer ownership table
is ADR-0009 lines 44–62 (`docs/decisions/0009-app-extension-kernel-boundary.md`);
the D0 rule (line 61): if your app needs a noun in `nmp-core`, the *kernel
boundary* is wrong, not the app.

## Per-seam checklist

Implement only the seams your protocol needs; `register_actions()` wires them
into the app's `NmpApp`. `nmp-nip29` registers 15 `ActionModule`s and one
snapshot projector for the group-chat read model. Minimum surface:

| Seam | Must provide | Reference |
|---|---|---|
| `ActionModule` | `NAMESPACE`, `type Action`, `start()`, `execute()` dispatching `ActorCommand` | `crates/nmp-nip29/src/action/mod.rs` |
| `register_snapshot_projection` | `snapshot_json() -> serde_json::Value` on the read model; registered under `nmp.<crate>.*` | `crates/nmp-nip29/src/register.rs:66` |
| `register_event_observer` | `KernelEventObserver` impl populating the read model from raw `KernelEvent`s | `nmp-app-chirp`'s observer pattern |
| `CapabilityModule` | request → native execution → typed result *envelope* (never `Result`) | [16 — Capabilities](16-capabilities.md) |

The unifying ownership rule a protocol crate states explicitly
(`nmp-nip29/src/kinds.rs`): "the kind is the dispatch; the `h` tag is
the ownership." Pick *one* such rule and document it in your `lib.rs`.

### How `nmp-nip29` wires its seams

`crates/nmp-nip29/src/register.rs`:

```rust
// Called by the host's FfiApp::new (or equivalent) during init.
pub fn register_actions(app: &mut NmpApp) {
    app.register_action::<PostChatMessageAction>();
    app.register_action::<ReactInGroupAction>();
    app.register_action::<CreatePublicGroupAction>();
    app.register_action::<DiscoverGroupsAction>();
    app.register_action::<JoinGroupAction>();
    // … 10 more ActionModules
}

// Called separately after the read model is constructed.
pub fn register_projector(app: &mut NmpApp, projection: Arc<GroupChatProjection>) {
    app.register_snapshot_projection("nmp.nip29.group_chat", move || {
        projection.snapshot_json()    // cheap, non-blocking
    });
}
```

Each `ActionModule` carries a typed `GroupId` routing key so `execute` can
call `send(ActorCommand::PublishUnsignedEventToRelays { relays: vec![group.host], … })`
— the planner gets `relay_pin: Some(host)` and routes to the group relay,
never the author's NIP-65 outbox (D3's third routing lane, ADR-0012).

## PR-ready file list

**Must add**

- `crates/nmp-nip<N>/Cargo.toml` — dep on `nmp-core` + `nmp-ffi` **only**
  (plus `serde`, protocol libs). Add the crate to the workspace `members`.
- `crates/nmp-nip<N>/src/lib.rs` — module layout + the boundary statement
  ("does NOT import any other `nmp-nip*`"; "`nmp-core` gains zero <noun>
  nouns") + public `register_actions(app: &mut NmpApp)` fn.
- `src/<protocol_id>.rs` — the typed routing/identity key (cf.
  `group_id.rs`, 117 LOC: `GroupId { host_relay_url, local_id }` + codec).
- `src/kinds.rs` — kind constants + a dispatch helper.
- `src/action/mod.rs` — `ActionModule` impls, one per protocol operation.
- `src/projection/` — the read model struct(s) + `snapshot_json()`.
- `src/tests.rs` (`#[cfg(test)] mod tests;` from `lib.rs`) **and** an external
  `tests/<lifecycle>.rs` proving the crate is a pure consumer of generic
  kernel APIs.

**May add**

- `src/interest.rs` — helpers building typed `LogicalInterest`s with
  `relay_pin` set (cf. `nip29/src/interest.rs:1-46`, `host_pinned_interest`).
- `src/cache/mod.rs` — protocol-local caches (TOFU signer, recent events).
- `src/moderation.rs` — audit/trust materialization, separate from canonical state.
- `docs/design/nip<N>-crate.md` + `docs/design/nip<N>/{routing,kinds,…}.md`.

**Must NOT add**

- Any `use nmp_nip01::*` / dep on another `nmp-nip*` (compose at the app layer).
- Any new variant/noun in `nmp-core` (no `Group`, no `enum GroupKind`).
- App-specific deps (no UI crates, no app config).
- Session-state mutation from the protocol crate.

## When a kernel change is justified — the `relay_pin` rubric

Some protocols route inverted relative to NIP-65: a subscription is bound to a
*host relay*, not the author's mailboxes. NIP-29 group events are the canonical
case. ADR-0012 (`docs/decisions/0012-relay-pinned-interest-and-third-routing-lane.md`)
weighed three shapes and shows the rubric:

1. **Reusable mechanism, zero protocol nouns.** The kernel got a generic
   `InterestShape::relay_pin: Option<RelayUrl>` field + lattice Rule 9 +
   partition Case E. Not `nip29_pin`, not a `Group` type — a protocol-agnostic
   carrier.
2. **Future consumers participate with zero compiler changes.** Other
   relay-pinned NIPs (livestream, closed-relay communities) set the same field.
   If your change only ever helps *your* protocol, it belongs in the protocol
   crate, not the kernel.
3. **The protocol crate is provably a pure consumer.** `nmp-nip29` only
   *populates* `relay_pin`; `crates/nmp-nip29/tests/lifecycle.rs` proves a
   hand-built generic interest produces the identical per-relay plan.
4. **Bypassing the planner** fails D1 (diagnostics blind), D8 (parallel REQs
   don't coalesce), and the framework-magic contract. Never hand-roll raw
   REQ/publish in a protocol crate.

Litmus: *a kernel change survives D0 iff it adds a reusable, protocol-agnostic
mechanism and the protocol crate that motivated it can be shown to be one
consumer among many.*

## Anti-patterns

- **`use nmp_nip25::*` inside `nmp-nip29`.** Protocol crates never import each
  other; cross-protocol composition (a NIP-25 reaction *on* a NIP-29 message)
  is the app crate's job.
- **A noun leaks into `nmp-core`.** Adding `Group`/`GroupKind` to the kernel
  "to make routing easier." The fix is a generic mechanism (`relay_pin`), not
  a protocol noun — re-derive your change against the four-step rubric.
- **Protocol crate owns policy.** Encoding ranking/feed/UX rules in the
  protocol crate. NIP-29 ships *audit-only* moderation — canonical membership
  is never mutated by the audit trail; policy on top is the app's.
- **Routing inferred from raw tags at plan time.** Every NIP-29 action takes a
  typed `GroupId` so `execute` gets `relay_pin: Some(host)` — it never parses
  `["h", …]` strings to derive routing.
- **Skipping the MockRelay integration test.** Without
  `tests/lifecycle.rs`-style proof that the crate is a pure consumer of the
  generic kernel API, a D0 regression ships unnoticed.
- **Protocol crate mutates session state.** Identity/account transitions are
  `nmp-signers` + the kernel's `AccountManager`; a protocol crate reads scope,
  never writes it.
- **Using removed v2 traits.** `DomainModule`, `ViewModule`, `IdentityModule`,
  `ModuleRegistry` are not on master. See [05a](05a-substrate-traits.md)
  §Removed v2 traits.

See also: [05a — Kernel substrate — traits + seams](05a-substrate-traits.md) ·
[07 — Subscription planner](07-subscription-planner.md) ·
[15 — Codegen — `nmp gen modules`](15-codegen-and-ffi.md) ·
[18 — Testing](18-testing.md) ·
[22 — Doctrine compliance checklist](22-doctrine-checklist.md)
