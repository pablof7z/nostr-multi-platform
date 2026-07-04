---
title: "The Write Pipeline: Construction, Signing, Publishing"
slug: write-pipeline
topic: write-pipeline
summary: "The 'door' metaphor refers to the app-facing API surface: the read door is how an app consumes data (typed read sessions), and the write door is how an app prod"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-29
updated: 2026-07-04
verified: 2026-06-29
compiled-from: conversation
sources:
  - session:898a41b5-68e0-4b0f-b16c-c6072454bd6a
  - session:dcc80382-bcc0-45ea-8b9c-1a2fc741f872
---

# The Write Pipeline: Construction, Signing, Publishing

## Write Door

The 'door' metaphor refers to the app-facing API surface: the read door is how an app consumes data (typed read sessions), and the write door is how an app produces and emits Nostr events. Every user action that creates a new event on the network flows through the write door — post, reply, react, repost, follow/unfollow, edit profile, send DM, join/post in group, delete, mute.

The shell (Swift/Kotlin/TS) is thin on purpose: it builds a typed action and renders status. Routing, signer choice, retries, durable write state, and provenance are all Rust-owned.

<!-- citations: [^898a4-cf36b] [^898a4-0e196] [^898a4-0ec98] -->
## Pipeline Stages

The write pipeline is: the app states intent as a typed action builder, encoded into a FlatBuffers `DispatchEnvelope`, pushed through the single `dispatch_action` byte doorway, routed to the per-NIP `ActionModule::execute()`, which mints a local publish intent (status: pending), constructs the unsigned event, signs via a generic backend-invisible capability, then publishes with route provenance, streaming status back via `ViewBatch` snapshots.

The pipeline separates construction, signing, and publishing into three distinct phases, enforcing the doctrine that signing and publishing must be orthogonal. Construction, signing (generic capability — local nsec, NIP-46 bunker, or hardware — backend-invisible to the protocol worker), and publishing (separate, routed, provenance-classified) are distinct steps.

<!-- citations: [^898a4-a8ec9] [^898a4-7ff83] [^898a4-dcb20] -->
## Dispatch ≠ Success

Dispatch is not success — handing an event to the publish pipeline is not the same as it landing. Rust owns a local publish-intent/status fact that progresses through states: pending → signed → stored → planned → sent → failed/exhausted. A write in NMP is a durable, observable status fact with a lifecycle, not a fire-and-forget function call. For replaceable event kinds (kind:3 unfollow, kind:0 profile-edit, kind:6 repost), the pipeline must actually publish to the relay — the Outbox must not falsely report 'All published' when the relay was never updated. Replaceable writes are held to the same durable-success bar as any other write: the status fact reflects real relay acceptance, not a local-only assumption.

<!-- citations: [^898a4-afd9e] [^898a4-82a63] [^dcc80-86789] -->
## Route Provenance

Every publish must declare explicit route provenance. A publish can never silently route without declared provenance. <!-- [^898a4-4c843] -->

## WRITE Constraints

WRITE-001 bans `PublishRaw` from being taught as the default write door in starter/DX paths; the generic raw publish seam remains underneath as the substrate but is not the app-facing API.

WRITE-002 makes publish finalization a named pre-sign gate, enforcing that dispatch is not conflated with success.

WRITE-003 replaces a loose optional `signer_pubkey` in publish commands with a typed `SignerProvenance` enum, with structured failure for unknown signers.

WRITE-004 deletes the anonymous route default (`Default for PublishRouteClass`) so a publish can never silently route without declared provenance.

WRITE-005 restricts pre-signed verbatim publish (`publish_signed_event`) to protocol-owned seams (e.g. Marmot/MLS wire events); it is not a general app write door.

<!-- citations: [^898a4-5df19] [^898a4-cccb9] -->
