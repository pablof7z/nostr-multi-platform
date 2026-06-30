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
| Can it declare an app-private Nostr kind? | no — reusable kinds only | yes — schema + contract live with the app |

Heuristic: a protocol crate is a **reusable mechanism**; an app core is
**policy + app nouns**. "I might reuse this someday" does **not** justify a
protocol crate — concrete cross-app reuse does. The four-layer ownership table
is ADR-0009 lines 44–62 (`docs/decisions/0009-app-extension-kernel-boundary.md`);
the D0 rule (line 61): if your app needs a noun in `nmp-core`, the *kernel
boundary* is wrong, not the app.

A protocol crate may be created for an app-requested need only after the
request has been reduced to a generic Nostr mechanism. Do not add app-named
actions, app-specific ranking or preview policy, catalog/network lookups, media
playback state, or one-product compatibility shims to `crates/`. If the
implementation would still make sense with the app name removed and a second
unrelated Nostr app consuming it, it can be a protocol/substrate crate.
Otherwise it belongs in the app core.

## App-private kind contracts are not protocol modules (#2408)

An app-private kind is the app-core path for product-specific Nostr data. It is
not a reason to create `crates/nmp-<app>` or to push the kind into `nmp-nip*`.
The app Rust crate owns the schema, semantics, validation, tag policy, publish
intent, `ActionPayload`, and `ActionModule`; NMP tooling projects that contract
into typed builders, native/web bindings, and drift checks.

Keep the contract next to the app Rust crate and FlatBuffers schema. It must
declare:

- `DispatchEnvelope.action_namespace`;
- the event kind number and publish-vs-local-work behavior;
- FlatBuffers schema path, root type, file identifier, schema id, and schema
  version;
- generated builder method name and flat-table field list/order;
- owning Rust crate/module/type names for `ActionPayload` and `ActionModule`;
- Swift, Kotlin, and TypeScript generated-builder output targets;
- app CI drift/check commands.

The contract is static codegen input. Do not build runtime schema loading,
dynamic module discovery, a plugin marketplace, a generic tag ontology,
generated composition roots, or automatic read model/projection generation for
an app-private kind. The app's explicit composition root still registers its
own `ActionModule` and read surfaces.

Use `crates/nmp-example-login-timeline` as the starter proof before creating a
new protocol crate. Its `action-builders.json` declares
`app.login_timeline.publish_status` for kind `30444`; its app-owned schema,
generated Swift/Kotlin/TypeScript builders, `PublishStatusAction:
ActionPayload`, and `PublishStatusModule: ActionModule` all live inside the
example app. The generated builders produce typed `DispatchEnvelope` bytes for
the app-owned decoder and module. They do not use a raw publish escape, and the
namespace is deliberately absent from the built-in NMP `ACTION_CONTRACT` and
`ACTION_BUILDERS` tables.

Promotion rule: move a private kind into an NMP protocol crate only after it is
usable unchanged as a generic Nostr mechanism by unrelated apps. Until then,
the kind remains app-owned even though it gets NMP-grade generated builders and
drift checks.

## Per-seam checklist

Implement only the seams your protocol needs; `register_actions()` wires them
into the app composition root through `AppHost`. `nmp-nip29` registers 15
`ActionModule`s and typed read-session/output support for the group-chat read
model. Minimum surface:

| Seam | Must provide | Reference |
|---|---|---|
| `ActionModule` | `NAMESPACE`, `type Action`, `start()`, `execute()` dispatching `ActorCommand` | `crates/nmp-nip29/src/action/mod.rs` |
| typed read-session helper | demand, replay, status, output, dynamic source wakeups, and teardown | NIP-29 group feed registration |
| typed output transport | typed encoder on the session/read model, registered under `nmp.<crate>.*` | `crates/nmp-nip29/src/register.rs` |
| observed delivery | internal executor machinery only, declared after shape/scope/owner/replay | NIP-29 group feed internals |
| `CapabilityModule` | request → native execution → typed result *envelope* (never `Result`) | [16 — Capabilities](16-capabilities.md) |

The unifying rule a protocol crate states explicitly
(`nmp-nip29/src/kinds.rs`): NIP-29 is a *kind-blind transport* — it owns only
the `h` / `previous` / host-pin routing envelope and its own 9xxx/3900x kind
namespace, never a foreign kind. An `h` tag makes an event *routable into a
group*, not NIP-29's to own. Pick *one* such boundary rule and document it in
your `lib.rs`.

### How `nmp-nip29` wires its seams

`crates/nmp-nip29/src/register.rs`:

```rust
// Called from an app-core composition root during init.
pub fn register_actions(app: &mut impl AppHost) {
    // The SOLE kind-agnostic write surface; per-kind events (kind:7 reactions,
    // kind:16 reposts, …) are built by their owning NIP/app and routed through
    // this envelope — NIP-29 never names a kind.
    app.register_action(PublishGroupEventAction);
    app.register_action(CreatePublicGroupAction);
    app.register_action(DiscoverGroupsAction);
    app.register_action(JoinGroupAction);
    // … lifecycle/admin ActionModules. NO per-kind named action: a foreign
    // kind (kind:7 reaction, kind:16 repost) is built by its owning NIP and
    // routed through PublishGroupEventAction, never named here.
}

// Called separately after the read model is constructed.
pub fn register_projector(app: &mut impl AppHost, projection: Arc<GroupEventsProjection>) {
    app.register_typed_snapshot_projection("nmp.nip29.group_events", move || {
        projection.typed_snapshot()    // cheap, non-blocking
    });
}
```

Each `ActionModule` carries a typed `GroupId` routing key so `execute` can
call `send(ActorCommand::PublishUnsignedEventToRelays { relays: vec![group.host], … })`
— the planner gets `relay_pin: Some(host)` and routes to the group relay,
never the author's NIP-65 outbox (D3's third routing lane, ADR-0012).

## Default typed action contract

If a reusable protocol action is wired by `nmp-defaults`, add one
`ActionContract` row in `crates/nmp-codegen/src/action_contract/table.rs`.
That row is the source for the default action surface; do not duplicate these
facts in tests, generated builders, or docs.

The row must name the action namespace, producer crate/module, public payload
type, `ActionPayload::SCHEMA_ID`, schema path, FlatBuffers `root_type`,
schema version, file identifier, default tier, builder support, public
re-export policy, and typed-dispatch policy. The namespace must equal
`ActionModule::NAMESPACE`; the schema id/version must equal the public
`ActionPayload`; and the `.fbs` file must declare the same `root_type` and
`file_identifier`.

For a default action, also re-export the payload from
`nmp-defaults::action_payloads`. If host builders should be generated, mark the
contract `GeneratedFlatTable` and add the builder field shape in
`crates/nmp-codegen/src/action_builders/registry.rs`; the codegen tests fail if
either side is missing. JSON-only defaults require a tracked
`TypedDispatchPolicy::Exempt { issue }` row, not a local allowlist.

Before opening the PR, run the contract gates:

```bash
cargo test -p nmp-codegen action_contract
cargo test -p nmp-defaults --test action_contract
cargo test -p nmp-defaults --test typed_only_action_doorway_gate
cargo run -p nmp-codegen -- gen action-contract-report
```

App-private kind contracts do not get added to the default NMP action contract
table merely to obtain codegen. They are app-local contract inputs, and the
app's CI owns its drift gate.

## PR-ready file list

**Must add**

- `crates/nmp-nip<N>/Cargo.toml` — dep on `nmp-core` and reusable protocol
  dependencies only
  (plus `serde`, protocol libs). Add the crate to the workspace `members`.
- `crates/nmp-nip<N>/src/lib.rs` — module layout + the boundary statement
  ("does NOT import any other `nmp-nip*`"; "`nmp-core` gains zero <noun>
  nouns") + public `register_actions(app: &mut impl AppHost)` fn.
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
- **Bypassing the shipped seams.** Protocol crates use `ActionModule`, typed
  read-session helpers, typed output registration, and capabilities. Observed
  delivery remains internal executor machinery behind those helpers.

See also: [05a — Kernel substrate — traits + seams](05a-substrate-traits.md) ·
[07 — Subscription planner](07-subscription-planner.md) ·
[15 — Codegen: bindings + FFI surface](15-codegen-and-ffi.md) ·
[18 — Testing](18-testing.md) ·
[22 — Doctrine compliance checklist](22-doctrine-checklist.md)
