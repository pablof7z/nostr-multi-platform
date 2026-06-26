# ADR-0043: `nmp-blossom` protocol crate — idiomatic Blossom uploads via the `ProtocolCommand` seam

Status: Implemented

> Shipped in `crates/nmp-blossom/` with the module layout below. The
> backend-transparent `ActorCommand::SignEventForAccount` port is live
> (`crates/nmp-core/src/actor/dispatch.rs`,
> `crates/nmp-core/src/actor/continuations.rs`), and the kind:24242 auth builder
> uses a 5-minute TTL (`AUTH_TTL_SECS`, `crates/nmp-blossom/src/auth.rs`).

## Context

Blossom (BUD-02) lets a client PUT a binary blob (avatar, podcast artwork,
shake-feedback image/audio) to one or more blob servers, authorised by a
short-lived signed **kind:24242** Nostr event placed in an
`Authorization: Nostr <base64(event)>` header. The blob descriptor the server
returns (`url`, `sha256`, `size`, `type`, `uploaded`) is then referenced from
profile/event metadata.

Today the podcast-player would have to hand-roll this in app Rust:

- build a kind:24242 auth event (with the `x` = sha256(blob) tag),
- sign it (impossible for NIP-46 bunker users without raw key bytes — D13),
- base64-encode it, construct the header, stream the multi-MB blob over HTTP,
- parse the descriptor, and reconcile multiple servers.

That is exactly the "app hand-rolls sign-for-return + HTTP" pattern the
framework thesis exists to eliminate. NMP's per-protocol crates own the full
**Build → Sign → Transport** pipeline; apps dispatch a typed action and read a
result from a projection. `nmp-nip57` already proves an NMP protocol crate can
own an HTTP round-trip idiomatically through the open
`ActorCommand::Protocol(Box<dyn ProtocolCommand>)` seam without `nmp-core`
importing any HTTP crate or learning any protocol noun (D0).

This ADR supersedes the conservative stance in
`docs/wiki/blossom-upload-signing.md` ("NMP must not absorb Blossom HTTP; that
is per-app") and corrects `docs/wiki/nmp-app-podcast.md:26` ("HTTP transport
remains in the sibling podcast crate"). That stance predated the
`ProtocolCommand` seam landing and the V-41 NIP-57 LNURL migration that
demonstrated it. Protocol-crate HTTP is now idiomatic; Blossom transport
belongs in an `nmp-blossom` protocol crate, **not** in `nmp-core` and **not**
in each app.

### Two facts that make Blossom different from the NIP-57 zap precedent

1. **The event is not known until after an off-thread hash.** A zap request
   (kind:9734) is fully built before any HTTP happens, so NIP-57 signs on the
   actor thread *then* spawns the HTTP worker. A kind:24242 auth event carries
   the `x` tag = sha256 of the blob, which for a multi-MB file MUST be computed
   off the actor thread (D8). So the build+hash step is in the worker, and
   signing therefore happens **after** the worker has started.

2. **NIP-57's signing step is itself defective for bunker accounts.**
   `FetchLnurlInvoiceCommand` signs via
   `ProtocolCommandContext::active_local_keys()`, which returns `None` for
   NIP-46 bunker accounts — so zaps from a bunker account fail closed
   (ADR-0026 "Phase 1" limitation). A peer fix is in flight under **V-78
   bunker zap signing in lnurl**. NIP-57 is therefore a clean template for the
   HTTP/`ProtocolCommand` *plumbing* but an **example of the defect** for the
   *signing* step. This ADR specifies the correct, backend-transparent signing
   port and states explicitly that the same port fixes the V-78 class of bug
   (one correct seam, two consumers).

## Decision

Create **`nmp-blossom`**, a Layer-4 protocol crate (same shape and layer as
`nmp-nip57`), as the single home for Blossom transport. `nmp-core` learns
nothing about Blossom; it only routes the boxed `dyn ProtocolCommand` and gains
one generic, backend-transparent signing capability (below) that is not a
Blossom noun.

App-facing contract: dispatch `nmp.blossom.upload`, read a blob descriptor
from the `action_results[correlation_id]` projection. No HTTP, no base64, no
header construction, no continuation-scanning in app code.

### Decision 1 — Binary payload seam: filesystem path

The action payload carries a **filesystem path string** the app already wrote
the blob to; `nmp-blossom`'s worker opens it, streams it through a SHA-256
hasher, builds the kind:24242 with the resulting `x` tag, and streams the same
file as the PUT body.

Rejected alternatives:

- **base64-in-JSON** — a multi-MB blob cannot ride the action envelope.
- **registered bytes-slot handle** (FFI populates a buffer, action references
  it by id) — forces a new *mutable byte registry* into `nmp-core`, which is
  substrate surface that exists only to serve a protocol crate. D0 cost with no
  v1 benefit.

The path option keeps `nmp-core` touching **zero bytes** and needing **zero new
payload seam** — the path is just a string in the existing JSON envelope. It
serves both target consumers: the iOS/macOS FFI host writes to its app
container and passes the path; a pure-Rust shell writes a temp file. (WASM has
no filesystem, but WASM is out of v1 scope; the schema reserves a future
`blob_handle` one-of field so a streaming/IndexedDB seam can be added without a
breaking change.)

**SHA-256 is computed by the worker, streaming, off the actor thread — never by
the kernel.** The kernel never sees the blob.

### Decision 2 — Signing: a uniform, backend-transparent sign-account port (load-bearing core change)

**Signer transparency is non-negotiable.** Whether the selected signer is a
local nsec or a NIP-46 bunker MUST be invisible to the Blossom worker. The
worker MUST NOT reach for `active_local_keys` and sign locally — that is the
defective NIP-57 pattern (V-78). If the backend made any difference to the
worker, the signer interface would be wrong, and the fix is to fix the signer
interface, not to document a Blossom limitation.

**The worker does not sign at all.** Signing requires `&IdentityRuntime`
(actor-thread-only) and the bunker path is asynchronous (the broker round-trip
is parked on the idle loop). So the design is a **two-leg worker with a sign
hop on the actor thread in between** — the worker is identical for local and
bunker because both resolve through the same kernel-side nonblocking sign +
park machinery that already exists (`sign_with_account_nonblocking` →
`SignerOp` → `PendingSign`/`PendingSignReturn` idle-loop drain). Local resolves
`Ready` on the spot; bunker resolves `Pending` and is parked; the worker code
is unaware of which.

**New substrate capability (the one load-bearing `nmp-core` change):** a
generic, async-capable "sign this unsigned event with account X, then run this
continuation with the `SignedEvent`" port available to a `ProtocolCommand`. It
generalizes the existing `SignEventForReturn` path (which today resolves a
parked bunker sign into the `signed_events` projection) by replacing the fixed
projection write with a **boxed continuation** the kernel invokes when the
signature lands — whether inline (local) or later (bunker, from the idle-loop
drain). Concretely, an `ActorCommand` carrying `{ unsigned, signer_pubkey,
continuation: Box<dyn FnOnce(Result<SignedEvent, _>) + Send> }`, dispatched by
the worker via the cloned `Sender<ActorCommand>`:

- On the actor thread, the kernel calls `sign_active_nonblocking` (when
  `signer_pubkey` is `None`) or `sign_with_account_nonblocking` (when `Some`) —
  the exact functions the publish path already uses, which look across BOTH
  local keys and remote signers.
- `Ready` → the continuation is invoked immediately on the actor thread.
- `Pending` → parked exactly like `PendingSignReturn`; the idle-loop drain
  invokes the continuation when the broker turns the request around (or on
  timeout, with an error).

The Blossom worker's continuation re-enters the HTTP leg: it base64-encodes the
signed event, builds the `Authorization: Nostr …` header, and spawns the PUT
worker. **The same port is what V-78 should use to fix NIP-57's bunker-zap
bug** — one correct seam, two consumers. `active_local_keys()` is no longer the
signing entry point for any `ProtocolCommand`; it remains only for callers that
genuinely need raw local keys (and those are the ones V-78 retires for zaps).

`signer_pubkey: Option<String>` matches the in-flight publish-path field
(`PublishUnsignedEvent` / `PublishUnsignedEventToRelays` in
`crates/nmp-core/src/actor/mod.rs`) byte-for-byte: `None` = active account,
`Some(pubkey)` = the named roster key. Per-podcast NIP-F4 keys use
`nmp_app_register_agent_nsec`: they are persisted, app-managed local signer
slots that can sign by pubkey but are hidden from account projections and
cannot become active. Local-vs-bunker remains transparent for named keys
because `sign_with_account_nonblocking` handles both.

### Decision 3 — Blob-server list source: payload-supplied for v1; kind:10063 ingest is the Phase-2 idiomatic answer

For v1 the `nmp.blossom.upload` payload carries the explicit `servers` list.
This unblocks the podcast-player immediately with zero new ingest/cache/projection
surface.

The idiomatic long-term source is the user's **BUD-03 kind:10063** server-list
event — the Blossom analog of NIP-65 (kind:10002) and NIP-17's kind:10050. When
adopted it follows the exact `nmp-nip17` pattern: `nmp-blossom` registers a
kind:10063 ingest parser via `EventIngestDispatcher`
(`register_ingest_parser_kind(10063, …)`) into a crate-owned cache, and the
upload action reads that cache when `servers` is omitted. The router never sees
kind:10063 (it is not a relay-routing concern). This is deferred so v1 ships
the minimum that unblocks the app. See flagged user-decision Q2.

### Decision 4 — Return shape: ride the existing `action_results` projection (no Blossom noun in core)

The blob descriptor surfaces via the **existing** `action_results[correlation_id]`
projection — the same drain-once, correlation-keyed surface that
`RecordActionSuccess` / `RecordActionFailure` already feed. No new
`blossom_uploads` projection key is added to `nmp-core` — "blossom" is a
protocol noun and must not appear in the substrate (D0).

> Implementation note (resolved during this pass): the success terminal needs
> to carry a structured JSON descriptor, not just `ok:true`. If the current
> `RecordActionSuccess` carries no payload, the minimal generic extension is a
> success terminal that accepts an opaque `result_json: Option<String>` the
> protocol crate supplies — core never parses it. This is a generic capability
> (any action can attach a result), not a Blossom feature. Engineer to confirm
> the exact success-terminal payload shape and extend generically if absent.

Single-server success (the descriptor is the BUD-02 response body):

```json
{ "ok": true,
  "result": {
    "url": "https://blossom.example/<sha256>.png",
    "sha256": "<64-hex>",
    "size": 20480,
    "type": "image/png",
    "uploaded": 1733356800
  } }
```

Multi-server mixed/partial outcome mirrors `PublishOutcome::Mixed` thinking —
some servers accept, some fail; the action is `ok:true` if **at least one**
server accepted, and each server's outcome is itemised:

```json
{ "ok": true,
  "result": {
    "sha256": "<64-hex>", "size": 20480, "type": "image/png", "uploaded": 1733356800,
    "servers": [
      { "server": "https://b1.example", "ok": true,  "url": "https://b1.example/<sha256>.png" },
      { "server": "https://b2.example", "ok": false, "error": "413 Payload Too Large" }
    ] } }
```

All-fail is a `RecordActionFailure` with the aggregated reason
(`{"ok":false,"error":"all 2 servers rejected the upload: …"}`) so the host
spinner always resolves (D6).

### Decision 5 — v1 scope: upload only (BUD-02 PUT), namespace designed to extend

v1 specs **only** `nmp.blossom.upload` (BUD-02 PUT). The `nmp.blossom.*`
namespace is reserved so `nmp.blossom.mirror` (BUD-04), `nmp.blossom.delete`
(BUD-02 DELETE), and `nmp.blossom.list` (BUD-02 GET) drop in later as sibling
`ActionModule`s sharing the same crate, the same signing port, and the same
server-list source — no further core changes. (Binding directive from the
coordinator: scope = upload only for v1.)

### Decision 6 — Crate boundary alignment

`nmp-blossom` is a **Layer-4 protocol crate**, structurally identical to
`nmp-nip57`:

- Dependencies: `nmp-core` (substrate seams), `nmp-kinds` (the kind constant),
  `nostr` (rust-nostr — kind:24242 construction + signing; **never**
  reimplement crypto, per `docs/aim.md`), `ureq` (off-thread HTTP, same as
  NIP-57), `sha2` (streaming blob hash), `base64`, `serde`/`serde_json`.
- **Kind constant:** add `KIND_BLOSSOM_AUTH = 24242` to the Layer-0 `nmp-kinds`
  crate (workspace-canonical-kinds rule, V-57). `nmp-core::kinds` re-exports it
  via `pub use nmp_kinds::*`, but `nmp-core` carries no Blossom logic.
- **crate-boundaries.md follow-up:** `nmp-blossom` is not in the §2 Layer-4
  table because it is not a NIP. It is a new *leaf* protocol crate (like
  `nmp-nip51` was when added), so it does **not** disturb the 12-step §5
  migration order — it depends only on already-existing seams. A follow-up edit
  adds a §2 Layer-4 row and a §8 decision-log line. (That edit is out of scope
  for this ADR to avoid touching the canonical spec in a docs-only PR.)

## Module layout (mirrors `nmp-nip57`)

```
crates/nmp-blossom/
  Cargo.toml
  src/
    lib.rs          register_actions(app) — registers UploadAction
    kinds.rs        re-export KIND_BLOSSOM_AUTH from nmp-kinds
    action.rs       UploadAction (ActionModule): UploadInput shape, start() validation,
                    execute() builds Protocol(BlossomUploadCommand) and returns
    auth.rs         pure kind:24242 builder (UnsignedEvent) + base64 Authorization
                    header construction — no I/O, unit-tested in isolation
    upload/
      mod.rs        BlossomUploadCommand (ProtocolCommand): run() spawns the
                    hash+build worker; the sign hop; the PUT worker; result
                    aggregation; action_results follow-ups
      http.rs       streaming PUT + descriptor parse helpers (blocking I/O,
                    worker-thread only) — mirrors nip57 lnurl/pay.rs split for
                    the 500-LOC file ceiling
```

## App-facing API

Expose a typed upload intent helper. Generated/bridge code encodes the
`nmp.blossom.upload` payload and sends the finished envelope through the byte
doorway:

```json
{
  "file_path": "/var/mobile/.../avatar.png",
  "content_type": "image/png",
  "servers": ["https://blossom.primal.net"],
  "signer_pubkey": "<hex-or-omitted>"
}
```

- `file_path` (required) — local path to the blob the app already wrote.
- `content_type` (optional) — MIME type; sniffed from extension if omitted.
- `servers` (required for v1) — explicit BUD-02 endpoints. (Phase 2: omit to
  use the user's kind:10063 list.)
- `signer_pubkey` (optional) — `None`/omitted signs with the active account;
  `Some(pubkey)` signs with the named roster key (per-podcast NIP-F4). Local vs
  bunker is transparent.

Read: `action_results[correlation_id]` (drain-once), shape per Decision 4.
`UploadAction::is_async_completing()` returns `true` — the host subscribes to
the action terminal, never blocks the dispatch return.

## Internal flow

```
UploadAction::execute(input, cid, send)
  → send(ActorCommand::Protocol(Box::new(BlossomUploadCommand{
        file_path, content_type, servers, signer_pubkey, correlation_id })))
  → returns immediately (D8)

BlossomUploadCommand::run(ctx):                          [actor thread]
  record Requested stage; capture created_at = ctx.now_secs()  (D7)
  spawn worker A (std::thread):                          [worker thread]
     stream file → sha256 (x tag) → size → content_type
     build UnsignedEvent kind:24242 (auth.rs, created_at injected)  (D8: off-actor)
     send(ActorCommand::SignEventForAccount{                ← NEW generic seam
            unsigned, signer_pubkey,
            continuation: |signed| { spawn worker B }})
  kernel signs (Decision 2):                              [actor thread]
     sign_active/with_account_nonblocking → SignerOp
       Ready (local)   → invoke continuation now
       Pending (bunker)→ park; idle loop invokes continuation on resolve/timeout
  worker B (std::thread):                                 [worker thread]
     base64(signed) → Authorization: Nostr header
     for each server: HTTP PUT blob body → parse descriptor   (D8: off-actor)
     aggregate outcomes
     send(RecordActionSuccess{cid, result_json}) or RecordActionFailure{cid, reason}
       → action_results[cid] projection                   [host reads]
```

`nmp-core` only ever sees `ActorCommand::Protocol(boxed)`, the generic
`SignEventForAccount` seam, and the generic action-result terminals. It imports
no HTTP crate and names no Blossom token. (D0)

## Implementation record

The work landed in the order below; the core change (step 1) was the only
substrate-touching step.

1. **[CORE — load-bearing] Add the backend-transparent sign-account port to
   `nmp-core`.** Add `ActorCommand::SignEventForAccount { unsigned,
   signer_pubkey: Option<String>, continuation: Box<dyn FnOnce(Result<SignedEvent,
   SignError>) + Send> }` (or an equivalent typed continuation). Dispatch arm:
   reuse `sign_active_nonblocking` / `sign_with_account_nonblocking`; `Ready` →
   invoke continuation inline; `Pending` → park in a generalized
   `PendingSignReturn` whose drain invokes the continuation instead of writing
   `signed_events`. Add the matching `ProtocolCommandContext` helper so a worker
   can `ctx`-construct + `send` it. Unit-test local (inline) and a mock bunker
   (parked → resolved) both reach the continuation with a `SignedEvent`. Confirm
   whether `RecordActionSuccess` carries a result payload; if not, extend it
   generically with an opaque `result_json: Option<String>`.
2. **Retarget V-78 onto the new port.** Coordinate with the in-flight V-78
   fix: NIP-57's `FetchLnurlInvoiceCommand` signing step should consume
   `SignEventForAccount` instead of `active_local_keys`, proving the seam with a
   second consumer and closing the bunker-zap bug.
3. **Scaffold `crates/nmp-blossom`** with the module layout above; add
   `KIND_BLOSSOM_AUTH = 24242` to `nmp-kinds`; wire `Cargo.toml` deps.
4. **`auth.rs`** — pure kind:24242 builder (`t=upload`, `x=<sha256>`,
   `expiration`, content) + base64 `Authorization` header. Unit tests, no I/O.
5. **`upload/http.rs`** — streaming PUT + BUD-02 descriptor parse, with response
   size caps and a per-upload HTTP timeout (mirror NIP-57's `LNURL_HTTP_TIMEOUT`
   / `MAX_RESPONSE_BYTES`). Worker-thread blocking I/O.
6. **`upload/mod.rs`** — `BlossomUploadCommand::run` two-leg worker + sign hop +
   multi-server aggregation + `action_results` follow-ups.
7. **`action.rs`** — `UploadAction` `ActionModule` (`NAMESPACE =
   "nmp.blossom.upload"`, `is_async_completing() = true`), `UploadInput` shape,
   `start()` validation (non-empty `file_path`, non-empty `servers`),
   `execute()` emits the `Protocol` command.
8. **`lib.rs::register_actions`** registers `UploadAction`; podcast-player
   avatar/artwork/feedback uploads call a typed upload helper rather than a
   raw transport helper.
9. **Crate-boundary docs** list `nmp-blossom` as the Layer-4 Blossom owner.

### `nmp-core` substrate extensions required (touch core — review with care)

- **`ActorCommand::SignEventForAccount` + continuation-park** (step 1) — the
  one structural addition. Generalizes `SignEventForReturn` /
  `PendingSignReturn` from "write the result to `signed_events`" to "invoke a
  boxed continuation," and exposes it to `ProtocolCommand` workers via a
  `ProtocolCommandContext` helper. Backend-transparent (local + bunker), the
  sole signing entry point for protocol-crate workers going forward.
- **(verify) generic `result_json` on the success terminal** — only if
  `RecordActionSuccess` cannot already carry a structured payload. Generic, no
  Blossom noun.
- **`KIND_BLOSSOM_AUTH = 24242` in `nmp-kinds`** (Layer 0) — re-exported by
  `nmp-core::kinds`; carries no logic.

No HTTP crate enters `nmp-core`. No Blossom token enters `nmp-core`.

## Consequences

- The podcast-player (and any future app) gets idiomatic Blossom uploads by
  dispatching one typed action and reading one projection — no hand-rolled
  sign-for-return, base64, header construction, or HTTP.
- D13 holds: raw keys never reach app Rust; `nmp-blossom` holds a `SignedEvent`,
  never a private key (the sign-account port returns a signed event, not keys).
  This is stricter than NIP-57's current `active_local_keys` clone.
- Signer transparency is structural: the Blossom worker is identical for local
  and bunker accounts. The same seam fixes the V-78 NIP-57 bunker-zap bug.
- D0/D6/D7/D8 hold: no NIP/Blossom nouns and no HTTP client in `nmp-core`;
  errors become kernel state (toast + action terminal); the kernel owns
  `created_at`; all hashing and HTTP run off the actor thread.
- `nmp-blossom` owns Blossom HTTP while `nmp-core` remains HTTP-free.

## Resolved decisions (formerly flagged questions)

- **Q1 — resolved:** the backend-transparent `SignEventForAccount` port was
  built as the shared seam. It is the sole signing entry point for
  protocol-crate workers and is signer-transparent (local + bunker).
- **Q2 — resolved:** v1 takes blob servers from the action payload. kind:10063
  (BUD-03) ingest stays deferred to Phase 2; no kind:10063 ingest exists in
  `nmp-blossom` today.
- **Q3 — resolved:** the kind:24242 `expiration` window is a fixed TTL
  (`AUTH_TTL_SECS`, 5 minutes) computed from the build `created_at`.
