# Protocol-Crate Purity and NIP-29 Kind-Blind Transport

> D-rules touched: D0 (scope clarification), transport boundary. Live issues: #2506 (ADR
> needed), #2511 (`nmp-nip09` owner), #2517 (`reply.rs` violation). See also
> `crate-layers-and-inversion.md`.

## 1. D0 scope is `nmp-core` only

D0 bans app nouns from `nmp-core`. The doctrine-lint gate
(`crates/nmp-testing/bin/doctrine-lint/rules/d0.rs`) exempts every `crates/*/src/` path that
is not `crates/nmp-core/`. Protocol crates — `nmp-nip29`, `nmp-nip25`, `nmp-planner`, etc. —
**may** name their own protocol nouns (`GroupId`, `ReactInGroupInput` inside `nmp-nip25`). D0
is not a blanket ban on protocol vocabulary in protocol crates.

What D0 *does* prohibit: pushing a protocol noun INTO `nmp-core` (a `Group` variant on a core
enum, a `nip29` branch in a core router). The fix is always a generic, protocol-agnostic
substrate mechanism, not a protocol noun in the kernel.

> Note: the **layer-inversion rule** (`crate-layers-and-inversion.md`) extends *display/render/
> aggregation* purity to all sub-L5 crates. That is distinct from D0's app-noun ban, which is
> core-only. A protocol crate may name its own kinds; it may not carry render-cards or foreign
> NIP semantics.

## 2. Protocol crates own mechanisms, not cross-protocol features

A protocol crate owns the minimal mechanism to implement its NIP. It does not own composite
features combining two or more NIPs. The test: can you state the crate's concern in one
sentence without naming another NIP? If not, the crate is doing too much.

- **Right:** `nmp-nip29` owns h-tag envelope injection + previous-tag chain + host-relay
  routing.
- **Wrong:** `nmp-nip29` owns "group reactions" (h-tag routing PLUS NIP-25 kind:7 semantics).

## 3. NIP-29 is a kind-blind transport

`nmp-nip29` knows ONLY:
- Its intrinsic kinds: 9xxx moderation/lifecycle (9000–9009, 9021, 9022), relay-signed
  metadata (39000–39003), and the group-native content kinds.
- The `h`-tag envelope: inject `["h", local_id]`, `["previous", …]`, set `relay_pin =
  Some(host)`.

It does NOT know kind:7 (NIP-25), kind:5 (NIP-09), kind:16 (NIP-18), kind:1111 (NIP-22), or
any other foreign kind.

The sole group-event publish surface is `PublishGroupEventAction` /
`nmp.nip29.publish_group_event`:

```
Input: { group: GroupId, kind: u32, content: String, tags: Vec<Vec<String>> }
Contract: injects ["h", local_id] and ["previous", …]; pins to host relay;
          rejects caller-supplied "h" or "previous" tags.
```

Any kind publishes into a group through this ONE door. There are no kind-named wrappers.

## 4. Cross-protocol composition belongs in the app crate

When a UI feature combines two protocols (reaction on a group message, repost into a group),
composition is the **app crate's** responsibility:

```
nmp-nip25:  build kind:7 event shape (e/p tags, content)
nmp-nip09:  build kind:5 deletion artifact
nmp-nip29:  inject h-tag envelope via publish_group_event
app crate:  sequence the steps, own the UX policy
```

NIP-29 never sees the word "reaction."

**The anti-pattern (PRs #2504/#2505):** adding `react_in_group` / `unreact_in_group` /
`share_event_in_group` / `repost_in_group` directly into `nmp-nip29` with inlined
`REACTION_KIND = 7` / `DELETE_KIND = 5`. The crate accepted foreign kinds, built foreign-NIP
event shapes, and grew kind-named namespaces. The correction (shipped via #2509/#2513): remove
them; route through the single `publish_group_event` door. The owner reaction to this class of
violation is severe — treat it as a hard stop. Kind-blindness is enforced in Rust by the
doctrine-lint `nip29_kind_blind` rule; the architecture scanner does not duplicate it.

## 5. Builder-vs-transport separation

The crate that owns a kind builds its events — no other crate, not even the transport that
routes them, assembles the protocol-specific tags or kind literal.

- `nmp-nip25` owns kind:7 `e`/`p` tags, content validation, reaction identity.
- `nmp-nip09` owns (pending #2511) kind:5 deletion grammar.
- `nmp-nip29` owns only the `h`/`previous` envelope and host-relay pin; it receives an
  already-constructed `(kind, content, tags)`.

**A foreign kind literal in a transport-crate `NAMESPACE` is the violation written in its
name.** If an `ActionModule::NAMESPACE` inside `nmp-nip29` contains a cross-NIP verb (`react_`,
`comment_in_`, `delete_in_`, `repost_in_`), the boundary is wrong.

## 6. Thin-adapter rule for `nmp-nipXX`

Protocol crates are thin adapters over `rust-nostr` primitives. Use `nostr::EventBuilder`,
`nostr::SingleLetterTag`, `nostr::Kind`, `nostr::Filter`. Never reimplement wire encoding,
event-ID hashing, schnorr signing, or filter grammar from scratch. Protocol crates never
import another `nmp-nip*` crate; cross-protocol composition belongs in the app crate.

## 7. Open violations to watch

| Issue | File | Violation |
|---|---|---|
| #2506 | — | No formal ADR for cross-protocol composition; `docs/design/nip29/kinds.md §4` stale framing contradicts current direction. |
| #2517 | `crates/nmp-nip29/src/reply.rs` | NIP-10 e-tag grammar (root/reply/mention) lives in nmp-nip29; `GroupEvent` carries `reply_to`/`root`. Foreign-protocol concept in a transport crate. |
| #2511 | — | kind:5 deletion construction scattered; `nmp-nip09` not yet created as the positive owner. |

## 8. Review blocking criteria

- A `nmp-nip*` crate has an `ActionModule::NAMESPACE` with a cross-NIP semantic verb.
- A `nmp-nip*` crate imports another `nmp-nip*` crate as a dependency.
- A foreign kind literal (`kind:7`, `kind:5`, `kind:16`, `kind:1111`) appears in `nmp-nip29`
  source outside a comment or test fixture.
- A protocol crate builds another protocol's event wire shape instead of delegating to the
  owner crate.
- A `nmp-nip*` crate reimplements crypto/event-building primitives instead of adapting
  `rust-nostr`.
