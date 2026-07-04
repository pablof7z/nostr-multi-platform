---
title: NIP-AD URL Resolution and AdResolutionPolicy
slug: nip-ad-resolution
topic: read-door
summary: NIP-AD resolution is app-configurable via an injected `AdResolutionPolicy`; the framework ships no default on/off/WoT decision, and the app picks at its composi
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-07-04
updated: 2026-07-04
verified: 2026-07-04
compiled-from: conversation
sources:
  - session:fb992e80-b32b-4673-b2c2-40e8044504ee
---

# NIP-AD URL Resolution and AdResolutionPolicy

## Policy Architecture

NIP-AD resolution is app-configurable via an injected `AdResolutionPolicy`; the framework ships no default on/off/WoT decision, and the app picks at its composition root — structurally identical to how apps choose whether to register a rich component for a content kind. The policy is a host-provided predicate trait with a single `should_auto_resolve` method (pure, sync, no network) that the content renderer consults before firing the AD resolver for a note. The content renderer never hardcodes when to fetch; it always delegates the decision to the predicate. <!-- [^fb992-dd22a] -->

Built-in `AdResolutionPolicy` values include `NeverAutoResolve` (content renderer never fetches; resolution only on explicit search/paste), `FollowsOnly` (auto-resolve only for notes authored by someone the user follows), `WebOfTrust { max_distance }` (auto-resolve within WoT distance N), and `Always` (shipped but flagged high-risk). `AdResolutionPolicy::WebOfTrust` and `FollowsOnly` are closure-generic to avoid an `nmp-wot` dependency in the `nmp-nip-ad` crate. <!-- [^fb992-a8e74] -->

Explicit search/paste AD resolution is never policy-gated — it is a distinct user action carrying nip05's existing trust model, and the policy gate applies only to the content-renderer entry point. Moment-2 (paste/search `AdCandidate` dispatch) is never policy-gated and runs in parallel with free-text search. <!-- [^fb992-5ea67] -->

## AdUrlState

`AdUrlState` is the fail-open state enum for AD URL resolution in the content pipeline, modeling `NotAttempted`, `Resolving`, `ResolutionFailed{at}`, and `Resolved{projection_key}`. The `ResolutionFailed` terminal state ensures that failed AD resolution does not hang the embed pipeline; the link renders immediately and resolution is a non-blocking upgrade. Most URLs (which are just URLs) render immediately as plain links while resolution proceeds asynchronously. <!-- [^fb992-dba4d] -->

## Wire Encoding

Every plain http(s) URL tokenizes to `WireNode::AdCandidateUrl` (`WireNodeKind` byte 22) instead of the prior `Url` wire node. Chirp (a separate repo) is not edited for the `AdCandidateUrl` arm; a filed issue (#2981) tracks that Chirp must add the `.adCandidateUrl` SwiftUI arm on re-pin as a plain-link baseline. <!-- [^fb992-91c9d] -->

## URL Resolution Semantics

AD URL resolution is multi-result first-class: a URL resolves to a live `{filter, relays}` collection query yielding 0..N events, never a single-pointer reduction, and never uses a limit. The NIP-AD resolver selects the well-known path entry matching the requested path rather than blindly taking the first entry, because the well-known endpoint returns all path entries regardless of the `?ad=` query parameter. The resolver URL-keyed, in-memory LRU cache uses ~6 h TTL on success and ~24 h TTL on failure. <!-- [^fb992-6e058] -->

## Relay Routing

AD relay routing uses `InterestShape.relay_pin` — one-shot, client-side routing only, never serialized onto the wire, never merged into the outbox/gossip relay model — reusing the existing relay-pinning vehicle that NIP-50 pinned search already uses, not a new D3 exception. <!-- [^fb992-7d4ec] -->

## open_ad_collection

`open_ad_collection` is the NIP-AD delivery doorway in `nmp-nip-ad` that turns a resolved `{filter, relays}` into per-relay one-shot relay-pinned `ReadDemand`s, producing a typed ADCL collection snapshot consumable by embed renderers and search results. It creates one `OneShot` relay-pinned `ReadDemand` per resolved relay (`{filter_json, relay_pin: Some(relay), lifecycle: OneShot, scope: Global, replay: Structural}`), keeping the full filter with no limit. Results are deduplicated by event id (first arrival wins) and ordered by `created_at` descending. It uses a typed ADCL FlatBuffers snapshot for its collection delivery, mirroring NIP-50's N50S typed output pattern. Per-row rendering is deferred to `nmp_content::resolve_embed_projection` rather than reimplementing rendering. When AD resolution returns empty relays, `open_ad_collection` fails open with an empty snapshot and a live handle, not an error. <!-- [^fb992-5e69c] -->

## End-to-End Verification

NIP-AD is proven live end-to-end on desktop: https://trellis.rs/legible resolves through `.well-known/nostr.json?ad=/legible` to a kind:30023 event rendered via `ArticleCard` with the title 'Stop Reading Entrails'. <!-- [^fb992-a5b76] -->
