---
title: Marmot NIP Implementation
slug: marmot-nip-implementation
topic: marmot
summary: The project uses rust-nostr's nip44 and nip59 implementations rather than hand-rolled crypto; the from-scratch nmp-nip44 crate was reverted and deleted
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-06-19
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:d27a4f61-511b-4086-845d-335493f9b464
  - session:cc7dc68a-1fcd-49fe-98be-198f17b6d59e
  - session:f22be978-ccc6-42dd-bad0-2b2d5aba2999
  - session:1c093fa5-0f0e-4dee-bf38-99781e763f13
  - session:45fcf96e-5b37-414f-a080-820b74a4e179
  - session:47203d35-d7c9-4c12-bc47-a40773d7acc2
  - session:95d02563-5473-4d84-96e1-cd342e1b04d1
  - session:1670fcb8-f275-498c-975b-8bd912331ded
  - session:9fc44c34-8e49-4959-91b3-714d4722ac3d
  - session:7b06d382-8fc2-4d52-bef5-f4d90e38cb2a
  - session:cd2b6122-2b7c-43fc-941b-c51e79ffc691
  - session:019edc02-a179-78c0-acff-398927481ea0
  - session:019edc13-83b1-7143-8631-b0e695ea4afd
  - session:11850f79-923f-4a2a-a921-a4b9bec47c6c
---

# Marmot NIP Implementation

## Cryptography and Protocol Foundations

The project uses rust-nostr's nip44 and nip59 implementations rather than hand-rolled crypto; the from-scratch nmp-nip44 crate was reverted and deleted. nmp-core/src/nip19.rs is rewritten as a thin adapter over `nostr::nips::nip19`, eliminating the parallel hand-rolled bech32/TLV codec (with u16 kind-overflow and >255-byte TLV guards). NIP-19 in nmp-core is a thin adapter over nostr::nips::nip19, preserving NMP's public API surface (Nip19Entity/NprofileData/NeventData/NaddrData/Nip19Error/encode_*/decode_*/parse/format) while deleting the hand-rolled bech32/TLV internals; the adapter adds guards for u32 kind > u16::MAX (now a typed Err instead of silent truncation) and >255-byte TLV length overflow (now a typed Err). The nip19 wire-golden drift in #1542 is byte-layout-only (identical coordinates) caused by canonical rust-nostr bech32 re-encoding differing from the old hand-rolled codec's TLV field ordering + RelayUrl normalization. nip21.rs and tags.rs are compliant kind-agnostic protocol codecs (module docs cite D0); no change beyond the nip19 dependency refactor. The nostr Rust crate must be reused for protocol codecs; protocol codecs must never be re-implemented from scratch. A hand-rolled Bech32/TLV codec is not acceptable solely because Bech32 is 'not crypto'; two parallel codecs in the same repo create correctness and divergence risk that violates the reuse rule. nmp-nip59 uses a WrapPlan carrier instead of nip29's PublishPlan because gift-wrap requires live key material for NIP-44 encryption, which PublishPlan cannot accommodate. WelcomeWrap uses a WrapPlan carrier for the same reason. nmp-nip59 uses futures::executor::block_on to bridge async nostr NIP-59 APIs to synchronous, pulling in no tokio dependency. PR-E Phase 1 introduced the SignerForSeal trait in nmp-nip59 with gift_wrap_with_signer, replacing identity.active_local_keys() with active_signer_for_seal() in dm.rs, and added a D13 lint banning raw-key access in DM/zap paths. Phase 1 deliberately does not build the RemoteSignerHandle → SignerForSeal adapter; bunker DMs still toast 'ADR-0026 Phase 2' and remain inert. Remote-signer NIP-44 support (Seam C) is explicitly deferred per the marmot plan; the local secret-key path works for now. ADR-0026 explicitly forbids reading marmot_local_nsec from DM/zap paths.

NIP-17 DM send is blocked for bunker (NIP-46 remote signer) accounts because the actor checks for local keys at dm.rs:87 and returns early if absent. NIP-17 DM receive also fails for bunker accounts because DmInboxProjection requires raw Keys for unsealing with no remote-signer path. RemoteSignerHandle::nip44_encrypt and nip44_decrypt exist as trait methods and NIP-46 routes them as RPC, but neither DM send nor receive calls them. nmp_nip59::gift_wrap calls nostr crate's native gift_wrap using only raw &Keys, never touching RemoteSignerHandle.

Bunker async-pending modeling will reuse the existing PendingSign machinery (actor/pending_sign.rs) and extend it to cover NIP-44 ops.

The DmInboxProjection already exists in crates/nmp-nip17/src/inbox.rs; reviews #45/#46 stating it was missing are superseded.

The D0 doctrine-lint bans `nip29` tokens in `nmp-core` but allows `nip17_*` field names based on precedent (`nip17_local_keys` already exists in `nmp-core`).

NIP-17 gift-wrap send is landed but receive-side cold-start is unverified.

Each NIP crate owns the kind constants it defines (NIP-59 owns 1059, NIP-17 owns 14, NIP-57 owns 9734/9735); higher-layer crates import from the protocol crate, never redefine locally. nmp-core is ~80.5k LOC acting as both substrate AND NIP runtimes (NIP-17, NIP-47, NIP-57, NIP-65); every NIP crate except nmp-nip02 and nmp-nip29 is split across two crates with half the logic in nmp-core. The kind:10050 DM-inbox cache lives in nmp-nip17 (the NIP crate that reads it), NOT in nmp-router; nmp-nip17 registers an IngestParser for kind:10050 that writes into its own DmRelayCache.

The empty relay hint in nmp-nip17 was assessed as intentional design per NIP-10 allowing optional relay hints in Phase 1 MVP.

Marmot key-package and group-message kind constants (30443, 443, 445) are collapsed from three separate sites (interest.rs as u32, service.rs as u16, projection/state.rs as u32) into one canonical u32 definition in nmp-kinds, re-exported via nmp-core. At Kind::Custom call sites, the canonical u32 kind constants are cast to u16 (`as u16`) because nostr::Kind::Custom takes u16; all Marmot values (30443, 443, 445) and NIP-60 values (17375, 7375, 7376, 7374, 10019, 9321, 38172) fit safely in u16 (< 65536). The public constants KIND_KEY_PACKAGE, KIND_KEY_PACKAGE_LEGACY, and KIND_GROUP_MESSAGE in nmp-marmot's interest module were removed and replaced by re-exports of KIND_MARMOT_KEY_PACKAGE, KIND_MARMOT_KEY_PACKAGE_LEGACY, KIND_MARMOT_GROUP_MESSAGE from nmp-core::kinds, breaking any downstream consumer that imported the old names. Hardcoded kind literals (443, 445, 1059, 30443) remain in nmp-marmot's projection/tap.rs and projection/ops.rs, so the 'single source of truth' claim is not yet fully realized.

The repost "triple-path" finding is stale; `nmp-nip18::try_from_kernel_event` is the canonical decoder, and the old kernel test-only `parse_repost_inner` helper has been deleted.

<!-- citations: [^11850-118] [^1c093-20] [^1c093-21] [^45fcf-7] [^d27a4-3] [^1c093-19] [^47203-11] [^95d02-11] [^1670f-8] [^cd2b6-7] [^019ed-14] [^019ed-71] [^11850-13] [^11850-34] [^11850-56] [^11850-72] [^11850-99] [^11850-209] [^11850-237] -->
## Marmot Crate Architecture and MDK Dependency

Chirp relies exclusively on the nmp Rust kernel for all data retrieval and networking, with no Swift-side networking or side channels. nmp-marmot wraps mdk-core 0.8.0 (Marmot Development Kit), not a custom MLS implementation. The D0 kernel boundary requires nmp-core to have zero MDK/openmls imports and zero MLS nouns. nmp-app-chirp has a direct mdk-core dependency as a typed translation layer, which deviates from the literal 'nmp-marmot is the sole importer' wording but not from the D0 kernel doctrine. The Marmot crate uses a two-layer design: stub substrate modules (zero MLS imports, satisfy registry wiring and kernel-boundary grep) plus a real MarmotService driving MDK. MDK pending-commit discipline requires calling commit() after create_group/add_members/self_update and clear() on publish failure; Drop provides a backstop. PR #460 (V-38 step 7: lift NWC wallet stack out of nmp-core into nmp-nip47) was merged to master.

When a new account is created on Chirp, it automatically follows npub1l2vyh47mk2p0qlsku7hg0vn29faehy9hy34ygaclpn66ukqp3afqutajft and fiatjaf's key by publishing an initial kind:3 contact list event.

NIP-29 discovery and join actions (nmp.nip29.discover, nmp.nip29.join) were implemented with DiscoverGroupsAction, JoinGroupAction, DiscoveredGroupsProjection, and iOS JoinGroupView, shipped as PR #210.

NIP-29 ships 3 group-chat ActionModule implementations (post, react, comment); 5 admin/membership executors were deleted as out of v1 scope. NIP-29 action namespaces use the 'nip29.' prefix instead of the 'nmp.' convention used by all other protocol crates (nip17, nip57, etc.).

NIP-17 action namespaces use the `nmp.nip17.*` convention (not the older `nmp.dm.*` shorthand).

The chirp://nip46 callback-scheme constant should eventually be Rust-owned (Phase-2 follow-up from OnboardingView+NIP46 PR).

The formal scope decision on Marmot/NWC inclusion in v1 remains open.

The MailboxCache (NIP-65 kind:10002 per-pubkey relay list cache) lives in nmp-router alongside the kind:10002 ingest parser that is its single writer.

Post-v1 backlog includes Cashu wallet support via NIP-60 and nutzaps via NIP-61 (no nmp-nip60/61 crates exist on master yet).

<!-- citations: [^9fc44-6] [^95d02-12] [^1c093-23] [^45fcf-8] [^f22be-1] [^d27a4-4] [^cc7dc-1] [^1c093-22] [^47203-12] [^1670f-9] [^7b06d-4] -->
## Publish Pipeline and Relay Routing

nmp_app_publish_signed_event publishes a pre-signed Nostr event verbatim through the routed relay pipeline without re-signing, requires no active account, and routes via the event's own pubkey kind:10002 outbox. nmp_app_publish_signed_event_to adds explicit relay targeting; a null/empty relays_json array falls back to PublishTarget::Auto (NIP-65 outbox), a non-empty array routes to exactly those relays. PublishTarget::Explicit already existed in the publish engine; the signed-publish path was threaded through it for kind:445 group messages to route to group-pinned relays rather than the author's outbox. Marmot dispatch ops publish internally via the kernel symbols rather than returning signed events for a non-existent Swift relay path; kind:445 routes to group-pinned Explicit relays, key-packages to Auto outbox, and gift-wrapped welcomes to group relays. nmp_core::store::VerifiedEvent::try_from_raw is the canonical Schnorr signature plus event-id hash verification primitive, shared between the ingest side and the signed-publish path. Gift-wrap inbox routing currently approximates to group relays because there is no NIP-65 inbox resolver; a future implementation is needed for reliable peer-to-peer invite delivery. <!-- [^d27a4-5] -->

## Inbound Ingest and Raw-Event Tap

The raw-event observer tap is a separate additive observer that does not mutate KernelEvent or touch the M2 subs/projection hot path. nmp_app_register_raw_event_observer accepts a kind filter (JSON array of u32 kinds) and a callback receiving verbatim flat NIP-01 signed event JSON with the sig field preserved. The inbound Marmot ingest seam is closed by a raw-event tap registered inside the Marmot projection that auto-feeds kind:1059/445 events through the existing ingest core; no iOS/Swift change was needed. The tap fires on the kernel actor thread and locks MarmotProjection's inner Mutex via with_inner; all errors are silent no-ops (D6 contract) and never panic across the FFI boundary. The Marmot raw-event tap was extended from kinds [444, 445, 1059] to [443, 444, 445, 1059, 30443] to cache inbound key-package events. <!-- [^d27a4-6] -->

## Key-Package Management

The key-package cache (kp_cache) lives in MarmotService in the shared nmp-marmot crate so all NMP apps benefit, not in nmp-app-chirp. KeyPackageLookupView is a ViewModule in nmp-marmot that triggers the kernel to fetch kind:30443/443 events for the specified author via NIP-65 relay routing. Solo group creation (no invitees) now proceeds normally instead of returning key_package_unavailable. Marmot key-package and group-message kind constants (30443, 443, 445) are collapsed from three separate sites (interest.rs as u32, service.rs as u16, projection/state.rs as u32) into one canonical u32 definition in nmp-kinds, re-exported via nmp-core. At Kind::Custom call sites, the canonical u32 kind constants are cast to u16 (`as u16`) because nostr::Kind::Custom takes u16; all Marmot values (30443, 443, 445) and NIP-60 values (17375, 7375, 7376, 7374, 10019, 9321, 38172) fit safely in u16 (< 65536). The public constants KIND_KEY_PACKAGE, KIND_KEY_PACKAGE_LEGACY, and KIND_GROUP_MESSAGE in nmp-marmot's interest module were removed and replaced by re-exports of KIND_MARMOT_KEY_PACKAGE, KIND_MARMOT_KEY_PACKAGE_LEGACY, KIND_MARMOT_GROUP_MESSAGE from nmp-core::kinds, breaking any downstream consumer that imported the old names. Hardcoded kind literals (443, 445, 1059, 30443) remain in nmp-marmot's projection/tap.rs and projection/ops.rs, so the 'single source of truth' claim is not yet fully realized.

The marmot slice emits raw `published`/`age_secs`/`stale` + new `is_registered:bool` from `KeyPackageStatus`; removes `bucket_age`/`render_subtitle`/`action_label`; both shells render via shared helpers. <!-- [^11850-119] -->

<!-- citations: [^d27a4-7] [^019ed-72] -->
## Testing and Cargo-Deny Considerations

The concatenated single-process publish→loopback→ingest→tap→decrypt E2E test was abandoned after two watchdog stalls; it was replaced with a verifiable per-seam coverage evidence map documenting each hop independently proven. cargo-deny pre-existing failures (BSL-1.0 from egui/arboard, OFL-1.1 from epaint, RUSTSEC advisories for instant and paste) are unrelated to MDK and not in scope for the Marmot milestone.

Three v1 unblocks are: DM cold-start verification, first-launch defaults, and zap round-trip verification.

V-63 (HIGH) tracks NIP-47 payment serialization using unwrap_or_default producing an empty string sent to relay, causing payments to silently never fire. V-72 (LOW) tracks LocalKeySigner silently coercing kind overflow to u16::MAX. The nip47 runtime hung-spinner finding (point 3, finding A) is stale; success-terminal recording was already implemented in PR #1211 via reconcile.rs. The wallet-poc sleep-loop finding (P5 finding 1) is stale; the crate/file was deleted by PR #1509.

<!-- citations: [^d27a4-8] [^95d02-13] [^cd2b6-6] [^11850-35] -->
## Testing and Cargo-deny Considerations

Three v1 unblocks are: DM cold-start verification, first-launch defaults, and zap round-trip verification.

V-63 (HIGH) tracks NIP-47 payment serialization using unwrap_or_default producing an empty string sent to relay, causing payments to silently never fire. V-72 (LOW) tracks LocalKeySigner silently coercing kind overflow to u16::MAX. Rate-limited NIP-47 responses are now classified as Transient (retries with exponential backoff, then FailedAfterRetries); pow failures remain classified as Permanent.

<!-- citations: [^1c093-24] [^11850-190] -->
