# Opus direction review #70 — 2026-05-22

Scope: full code re-read of nmp-core FFI, nmp-app-chirp wiring, the 4 live NIP
crates, KernelBridge.swift, ActorCommand surface, and the wire envelope path.

## TL;DR

The seam thesis (one `dispatch_action` C-ABI symbol, N typed `ActionModule`
impls) is winning where it ships — 11 live action namespaces dispatched from
iOS, all five reviews ago dormant NIP crates now have first consumers, and the
ADR-0027 single-call `register_action::<M>()` collapse is done. The wins are
real.

But the codebase has not learned that **shipped-but-inert is now the dominant
class of debt, not D0 violations**. WireDelta is the worst offender: emitted at
two call sites, discarded by Swift at line 503 of KernelBridge.swift, schema
versioned, consumer-typed, documented in three places. There is no consumer.
Same shape: `nmp-codegen/src/swift.rs` exists, emits Swift Codables
deterministically, has zero generated files in `ios/Chirp/`. KernelBridge.swift
is still 1992 LOC of hand-typed mirror.

These are not protocol problems. They are velocity problems — every PR that
extends WireDelta or the codegen-able surface is paying a tax on infrastructure
that has no live consumer.

## False positives from prior reviews — stop repeating

- **ADR-0027 dual seam** (reviews #49, #54): collapsed. `register_action_module`
  + `register_action_executor` are deleted (`crates/nmp-core/src/ffi/mod.rs:166-167`,
  `nmp-core/src/kernel/action_registry.rs:330`). Only `register_action::<M>()`
  remains.
- **Bunker DM send inert** (reviews #41, #47, #48): closed. `dm.rs` routes
  through `nmp_nip59::gift_wrap_with_signer` (`actor/commands/dm.rs:279`) which
  handles both LOCAL (synchronous) and REMOTE (NIP-46 bunker) signers via the
  `RemoteSignerForSeal` adapter.
- **ZapAction is a stub** (review #56): obsolete. `crates/nmp-nip57/src/action.rs:113`
  is a real executor that builds the kind:9734 and enqueues `FetchLnurlInvoice`;
  end-to-end zap shipped in commit `ffi63f64`.
- **last_action_outcomes scalar bug** (reviews #25, #29, #30): superseded.
  PR-G threaded correlation_id through executors; the spinner round-trip
  closes for PublishNote, react, follow, unfollow.

## 1. What NMP should support that it doesn't

### kind:10002 republish is missing — relay edits never propagate

`AddRelay`/`RemoveRelay` (`actor/dispatch.rs:617`, `:637`) mutate the local
`RelayEditRow` projection and dial sockets, but emit no kind:10002. The author's
NIP-65 outbox is only published once, on `create_account` cold-start
(`actor/commands/identity.rs:836-845`). After that, every relay edit makes the
user's published outbox more stale.

Concretely:
- User signs in, removes a defunct relay → no kind:10002 update → other clients
  routing to that user still fan out to the dead relay.
- User adds a new relay → that relay is never advertised → contacts never know
  to read/write there.

The pattern is solved for kind:10050 by `PublishDmRelayListAction` /
`nmp.nip17.publish_relay_list` (`crates/nmp-nip17/src/dm_relay_list.rs`). The
kind:10002 hole is the same shape — needs an `ActionModule` (~80 LOC) plus an
`AddRelay`/`RemoveRelay` post-hook that enqueues it (or a separate
`nmp.nip65.publish_relay_list` user action surface). Technically reachable
today via `PublishRaw { kind: 10002, ... }` through `nmp.publish`, but no UI
dispatches that and no `AddRelay` arm wires it.

This is the single most user-visible protocol hole and it has been open since
NIP-65 routing was added.

### NIP-25 reactions on group chat events stop at the namespace boundary

`ReactInGroupAction` (`nmp.nip29.react_in_group`) emits kind:7 with `["h", local_id]`
(group-pinned). `chirp.react` emits kind:7 NIP-65-routed for plain notes. There
is no path for a reaction on a kind:9 chat message *outside* the group's host
relay, and no path for a reaction on a kind:1 note that should also surface
inside a group context. The two namespaces are correct individually; the
seam between them is missing.

### Snapshot decode latency on every interaction

Every actor tick serializes the entire `KernelSnapshot` to one JSON string
(`kernel/update.rs:179`), pushes it to Swift, Swift parses it whole
(`KernelBridge.swift:494-540`). The `WireDelta` infrastructure exists but is
*intentionally discarded* by Swift (next section). For a feed with a few
hundred cards plus DM inbox plus group chat plus zaps, every tap-induced
snapshot rev is bounded by full snapshot decode. This was review #67's
finding and is unchanged.

## 2. What NMP does that it shouldn't

### WireDelta is shipped-but-inert end-to-end — DELETE the emit path

The most embarrassing infrastructure in the tree right now.

- Emit: `wrap_update` (`update_envelope.rs:173`) called from exactly two
  sites — `ActorCommand::Kernel` arm at `actor/dispatch.rs:814` and the
  `OpenView` enqueue at `actor/tick.rs:164`. Triggered by `OpenUri` and
  `OpenView` only.
- Receive: `KernelBridge.swift:503-512` explicitly drops every `{"t":"update",...}`
  frame:
  ```swift
  guard frameTag == "snapshot" else {
      if frameTag == "update" {
          kbLog.debug("discrete update frame received (not applied by snapshot bridge)")
      }
      return nil
  }
  ```
- Consumer types: `DeltaEnvelope`, `UpdateEnvelope::Update`, `DELTA_SCHEMA_VERSION`,
  `default_delta_schema_version`, the full schema-version comment block on the
  consumer side (`update_envelope.rs:124-143`).

That is a versioned, serialized, document-commented protocol with literally
zero consumers. It is a cargo-cult of an Elm architecture for which nobody is
willing to write the delta-apply code on the host. Either commit to it (write
the Swift delta-apply, get latency wins) or delete it. Status quo costs effort
on every PR that touches kernel update emission and pays for nothing.

### Marmot is still default-on, after six reviews said de-default

`apps/chirp/nmp-app-chirp/Cargo.toml:25`:
```toml
default = ["marmot", "wallet", "lmdb-backend"]
```

Loadout: 4,096 LOC Rust crate (`nmp-marmot`) + 1,270 LOC Swift surface
(`MarmotBridge.swift` 569 + `MarmotGroupsView.swift` 354 + `MarmotInviteSheet.swift`
+ `MarmotGroupChatView.swift`) shipped to every Chirp build.

Reviews #58, #65, #66, #68 all flagged this. The feature gate is wired
correctly (`marmot = ["dep:nmp-marmot", ...]`). One commit (`default = []` in
that file plus the matching Swift conditional compilation pass) removes ~5,400
LOC from the v1 critical path.

### `nmp_app_chirp_*` registration symbols still shadow `nmp_nip29::register::*`

PR #319 introduces `crates/nmp-nip29/src/register.rs` with `wire_group_chat`,
`wire_group_discovery`, `register_actions` — exactly the pattern the second app
needs. But `apps/chirp/nmp-app-chirp/src/ffi.rs:115-216` still calls all the
register helpers inline AND exposes `nmp_app_chirp_register`,
`nmp_app_chirp_register_group_chat`, `nmp_app_chirp_register_dm_inbox`,
`nmp_app_chirp_register_follow_list`, `nmp_app_chirp_register_group_discovery`
as C-ABI symbols. These read as Chirp-specific to a second app's eye, but they
are mostly NIP-crate generic wiring with a four-character `chirp` prefix that
makes them un-reusable. Either rename them (`nmp_nip29_wire_group_chat` etc)
or accept that Chirp is the only app and stop calling NMP a multi-platform
foundation.

### action_results / action_stages snapshot fields with zero iOS consumer

Review #24 worried about action_results being missing. It's now shipped
(`kernel/types.rs:181-188` documents the registration path). But:

```
$ grep -rn "action_results\|action_stages" ios/Chirp/Chirp/Bridge/
ios/Chirp/Chirp/Bridge/KernelBridge.swift:286
ios/Chirp/Chirp/Bridge/KernelBridge.swift:417
```

Only string-doc references; no actual decode of either field. The spinner UX
that the field was designed for (`PR-G`) is not closed end-to-end.

## 3. What could be better

### dispatch_action seam losing the write side — 21 bypass : 11 routed

Direct ActorCommand sends from `crates/nmp-core/src/ffi/*.rs`:

```
AddRelay, RemoveRelay, ClaimProfile, ReleaseProfile, CloseAuthor, CloseThread,
OpenAuthor, OpenThread, OpenFirehoseTag, OpenTimeline, RetryPublish,
CancelPublish, SignInNsec, SignInBunker, CreateAccount, SwitchActive,
RemoveAccount, WalletConnect, WalletDisconnect, WalletPayInvoice, Configure
```

That's 21 user-writeable verbs going around `dispatch_action`. The seam
captures: `nmp.publish`, `chirp.{react,follow,unfollow}`, `nmp.nip57.zap`,
`nmp.nip17.{send,publish_relay_list}`, `nmp.nip29.{post_chat_message,
react_in_group, comment_in_group, discover, join}`. Eleven.

The new `inflight_dispatches` generic idempotency guard (commit `9f30f912`,
`ffi/action.rs:271`) protects only the eleven, not the twenty-one. So:

- A user double-taps the "send DM" button: protected.
- A user double-taps the "switch account" button: not protected.
- A user double-taps "remove relay": not protected.
- A user double-taps "retry publish": not protected.

The dedup is namespace-keyed (`stable_hash(namespace, action_json)`); extending
it to the bypass commands would require either (a) re-routing those commands
through `dispatch_action` (real seam migration, larger PR), or (b) building a
parallel ActorCommand-keyed dedup. Option (a) is the right architecture; option
(b) closes the bug faster but cements the bypass.

### nmp-codegen produces nothing the iOS shell consumes

`crates/nmp-codegen/src/swift.rs` is a 262-LOC Swift Codable emitter — clean,
deterministic, byte-identical reproducibility. Nothing calls it on a path that
lands in `ios/Chirp/`. No `*.generated.swift` files exist. KernelBridge.swift's
hand-coded snapshot Codables (lines ~600-1992) are the actual contract.

For a project whose stated goal is multi-platform reusability, the absence of
generated bindings is the single biggest "second app would not get the same
benefits" indicator. A second app builds its own `KernelBridge.swift` from
scratch, hand-syncs every snapshot field rename, re-implements every dispatch
helper.

PR scoping suggestion: don't try to generate all of KernelBridge.swift at
once. Generate one type (e.g. the projection-payload structs for `nmp.nip29.*`
and `nmp.nip17.dm_inbox`) and prove the round-trip with a snapshot
conformance test.

### Singleton `swap_singleton_event_observer` slot — won't scale past one screen

`NmpApp::singleton_event_observer_id` (`ffi/mod.rs:278`) holds exactly one
auxiliary `KernelEventObserverId` for the host's "current group chat" or
"current discovery view". That's fine for Chirp v1 where one group chat view
is on screen at a time. It will fall over the first time:

- A "groups list" screen wants discovery on N relays in parallel.
- Two group chat views are stacked (e.g. swipe-back gesture mid-transition).
- A test harness drives N projections concurrently.

The handle-returning variant called out in the docstring comment is not
written. When it is, the slot becomes a generic id pool, not a single
`Mutex<Option<…>>`.

### `nmp_app_chirp_register` does too much for one entry point

That function registers chirp.react/follow/unfollow, NIP-29 actions, NIP-17
actions, NIP-57 actions, the DM runtime, the zaps aggregate projection, AND
the modular timeline projection — seven distinct cross-cutting concerns. A
second app inherits none of them or all of them via a giant copy-paste. The
right shape is the one PR #319 hints at: `nmp_nip29::register::all(app)`,
`nmp_nip17::register::all(app)`, `nmp_nip57::register::all(app)`, then the
app crate composes them.

## 4. The one thing I'd land in 48 hours

**Delete the WireDelta emit + consumer types.** ~150 LOC negative.

Why this and not kind:10002 (the bigger user hole):

1. The kind:10002 publish is ~80 LOC of new code on a well-understood pattern
   (mirror `PublishDmRelayListAction`). It's a Sonnet agent task. It's not a
   *direction* decision — it's a backlog item. Direction reviews exist to
   surface things only a senior eye spots.

2. The WireDelta deletion is a forcing function. Every previous review noted
   "Swift discards WireDelta"; every cycle the schema stays in tree and one
   more comment block ages. Deleting it ends the cargo cult. If somebody
   later wants delta-apply for latency, they will design it against the
   actual measured cost, not against a frame format that survived by
   inertia.

3. The deletion exposes the snapshot-per-tick cost honestly. Once the only
   wire shape is `wrap_snapshot`, the question "how do we cut snapshot
   decode latency" gets asked at the right level — projection-by-projection
   incremental decode in Swift, or revision-keyed `If-Modified-Since` style
   filtering — not at the level of a never-consumed delta channel.

4. It enforces the doctrine the project preaches. Shipping schema-versioned,
   consumer-typed protocol for nobody is the same anti-pattern as
   `nmp_app_publish_unsigned_event` was before PR-F deleted it. PR-F was the
   model. Apply it again.

**Concrete delete list:**
- `crates/nmp-core/src/update_envelope.rs:100-107` (`WireDelta`)
- `crates/nmp-core/src/update_envelope.rs:117` (the `Update(WireDelta)` arm)
- `crates/nmp-core/src/update_envelope.rs:134-143` (`DeltaEnvelope`)
- `crates/nmp-core/src/update_envelope.rs:152-156` (`UpdateEnvelope::Update`)
- `crates/nmp-core/src/update_envelope.rs:173-179` (`wrap_update`)
- `crates/nmp-core/src/actor/tick.rs:95-102` (`emit_kernel_update`)
- `crates/nmp-core/src/actor/dispatch.rs:809-817` (the `ActorCommand::Kernel`
  arm's `emit_kernel_update` call — keep the dispatch arm; replace the discrete
  emit with `maybe_emit_after_dispatch`)
- `ios/Chirp/Chirp/Bridge/KernelBridge.swift:503-512` (the `frameTag == "update"`
  branch + comment)
- All tests that decode `UpdateEnvelope::Update` (`update_envelope.rs:238-301`)

The `t=panic` and `t=snapshot` arms stay. The schema version stamp on snapshots
stays. The carrier enum becomes a two-variant `{Snapshot, Panic}` — same
discriminator-tagged shape, just without the dead third arm.

If somebody objects, the rebuttal is: PR #67 was right, every review since has
noted it, and a year-old "we'll consume it when we get to it" is not a delta
delivery path.

---

## Closing note

The project still spends review cycles enumerating dormant abstractions
(ViewModule, DomainModule, IdentityModule) it has already deleted. Reviews
#19-#33 cycled on that. The new dormant abstractions are subtler — WireDelta,
nmp-codegen, the `singleton_event_observer_id` slot, the
`nmp_app_chirp_register_*` family. The discipline that killed the explicit
dormant trait families needs to be turned on the implicit ones now.

The thesis is sound. The seam works. Stop adding things that "the second app
will use someday" until the second app exists.
