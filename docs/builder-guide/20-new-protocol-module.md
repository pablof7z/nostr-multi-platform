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

## Per-trait minimum-impl checklist

Implement only the families your protocol needs; `register()` wires them into a
kernel `ModuleRegistry`. `nmp-nip29` ships 13 Domain + 7 View + 15 Action
(`crates/nmp-nip29/src/{domain,view,action}/mod.rs`); a small protocol may ship
one of each. Minimum surface per family:

| Family | Must implement | Reference |
|---|---|---|
| `DomainModule` | `NAMESPACE`, `SCHEMA_VERSION`, `migrations()`, `indexes()`, `register()` | `fixture-todo-core/src/lib.rs:13-37` |
| `ViewModule` | `NAMESPACE`, 5 assoc types, `key`, `dependencies` (never empty — empty forces a table scan), `open`, `on_event_*`, `snapshot` | `docs/design/view-catalog/template-and-enumeration.md:1-60` |
| `ActionModule` | typed input carrying its routing key (NIP-29: a typed `GroupId`), durable ledger transition, no raw-tag routing inference | `crates/nmp-nip29/src/action/mod.rs:1-20` |
| `CapabilityModule` | request → native execution → typed result *envelope* (never `Result`) | section [16](16-capabilities.md) |
| `IdentityModule` | scope kind; no long-lived state | section [11](11-sessions-signers.md) |

The unifying ownership rule a protocol crate states explicitly
(`nmp-nip29/src/domain/mod.rs:6-9`): "the kind is the dispatch; the `h` tag is
the ownership." Pick *one* such rule and document it in your `lib.rs`.

## PR-ready file list

**Must add**

- `crates/nmp-nip<N>/Cargo.toml` — dep on `nmp-core` **only** (plus
  `serde`, protocol libs). Add the crate to the workspace `members`.
- `crates/nmp-nip<N>/src/lib.rs` — module layout + the boundary statement
  ("does NOT import any other `nmp-nip*`"; "`nmp-core` gains zero <noun>
  nouns") + a `register(&mut ModuleRegistry)` fn.
- `src/<protocol_id>.rs` — the typed routing/identity key (cf.
  `group_id.rs`, 117 LOC: `GroupId { host_relay_url, local_id }` + codec).
- `src/kinds.rs` — kind constants + a dispatch helper (cf. `kinds.rs`, 210
  LOC).
- One `src/<family>/mod.rs` per family you implement, each with a
  `register_all()`.
- `src/tests.rs` (`#[cfg(test)] mod tests;` from `lib.rs`) **and** an external
  `tests/<lifecycle>.rs` proving the crate is a *pure consumer* of generic
  kernel APIs (cf. `crates/nmp-nip29/tests/lifecycle.rs`).

**May add**

- `src/interest.rs` — helpers building typed `LogicalInterest`s (cf.
  `nip29/src/interest.rs:1-46`, `host_pinned_interest`).
- `src/cache/mod.rs` — protocol-local caches (TOFU signer, recent events).
- `src/moderation.rs` — audit/trust materialization, *separate* from
  canonical state.
- `docs/design/nip<N>-crate.md` + `docs/design/nip<N>/{routing,kinds,…}.md`.

**Must NOT add**

- Any `use nmp_nip01::*` / dep on another `nmp-nip*` (compose at the app
  layer).
- Any new variant/noun in `nmp-core` (no `Group`, no `enum GroupKind`).
- App-specific deps (no UI crates, no app config).
- Session-state mutation from the protocol crate.

## When a kernel change is justified — the `relay_pin` rubric

Some protocols route inverted relative to NIP-65: a subscription is bound to a
*host relay*, not the author's mailboxes. NIP-29 group events are the canonical
case. The M2 compiler's two-lane (Outbox/Inbox) model could not express it, so
a kernel change was unavoidable. ADR-0012
(`docs/decisions/0012-relay-pinned-interest-and-third-routing-lane.md`)
weighed three shapes and shows the rubric:

1. **Reusable mechanism, zero protocol nouns.** The kernel got a generic
   `InterestShape::relay_pin: Option<RelayUrl>` field + lattice Rule 9 +
   partition Case E. Not `nip29_pin`, not a `Group` type — a protocol-agnostic
   carrier. (Shape (a), a typed `RelayPinnedInterest` the compiler dispatches
   *by protocol*, was rejected: it couples the kernel to per-protocol nouns,
   violating D0.)
2. **Future consumers participate with zero compiler changes.** Other
   relay-pinned NIPs (livestream, closed-relay communities) set the same
   field. If your change only ever helps *your* protocol, it belongs in the
   protocol crate, not the kernel.
3. **The protocol crate is provably a pure consumer.** `nmp-nip29` only
   *populates* `relay_pin`; `crates/nmp-nip29/tests/lifecycle.rs` proves a
   hand-built generic interest produces the identical per-relay plan. The
   publish-side mirror (`PublishPlan { pin_to: Some(host) }`) lives in the
   **protocol crate** (`nip29/src/action/mod.rs:1-19`), not `nmp-core` — the
   kernel only knows the subscription-side field.
4. **Bypassing the compiler (Shape (c))** fails D1 (diagnostics blind), D8
   (parallel REQs don't coalesce), and the framework-magic contract. Never
   hand-roll raw REQ/publish in a protocol crate.

Litmus: *a kernel change survives D0 iff it adds a reusable, protocol-agnostic
mechanism and the protocol crate that motivated it can be shown to be one
consumer among many.* Otherwise keep it in the protocol crate (the typed
`GroupId`, the TOFU trust model in ADR-0013, the audit-only moderation policy
all stayed there).

## Anti-patterns

- **`use nmp_nip25::*` inside `nmp-nip29`.** Protocol crates never import each
  other; cross-protocol composition (a NIP-25 reaction *on* a NIP-29 message)
  is the app crate's job. `nmp-nip29` handles the `h`-tagged variant; the
  non-`h` form lives in `nmp-nip25`.
- **A noun leaks into `nmp-core`.** Adding `Group`/`GroupKind` to the kernel
  "to make routing easier." The fix is a generic mechanism (`relay_pin`), not
  a protocol noun — re-derive your change against the four-step rubric.
- **Protocol crate owns policy.** Encoding ranking/feed/UX rules in the
  protocol crate instead of mechanism only. NIP-29 ships *audit-only*
  moderation (`moderation.rs`, 82 LOC) — canonical membership is never mutated
  by the audit trail; policy on top is the app's.
- **Routing inferred from raw tags at plan time.** Every NIP-29 action takes a
  typed `GroupId` so the planner gets `pin_to: Some(host)` — it never parses
  `["h", …]` strings to *derive* routing (the only tag check is a structural
  refusal: an `h`-tagged event with no pin is rejected at construction).
- **Skipping the MockRelay integration test.** Without
  `tests/lifecycle.rs`-style proof that the crate is a pure consumer of the
  generic kernel API, a D0 regression (kernel quietly grew a noun) ships
  unnoticed.
- **Protocol crate mutates session state.** Identity/account transitions are
  `nmp-signers` + the kernel's `AccountManager`; a protocol crate reads scope,
  never writes it.

See also: [05 — Kernel substrate — the 5 trait families](05-substrate-traits.md) · [07 — Subscription planner](07-subscription-planner.md) · [15 — Codegen — `nmp gen modules`](15-codegen-and-ffi.md) · [18 — Testing](18-testing.md) · [22 — Doctrine compliance checklist](22-doctrine-checklist.md)
