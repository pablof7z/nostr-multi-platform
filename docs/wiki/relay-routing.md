---
title: NIP Crate Relay Routing Ownership
slug: relay-routing
topic: relay-routing
summary: NIP crates own relay routing for protocol-specific event kinds; apps pass only protocol identity (e.g., GroupId, recipient_pubkey), never relay URLs
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-03
updated: 2026-06-03
verified: 2026-06-03
compiled-from: conversation
sources:
  - session:13382c6f-c5ac-4856-99fd-3cbfdd0b06a5
  - session:7f143c67-6e46-424a-90a8-5bf844947fee
  - session:cf071d35-ee9b-4a1f-a3b8-885c651e8cce
---

# NIP Crate Relay Routing Ownership

## Relay Routing Ownership

NIP crates own relay routing for protocol-specific event kinds; apps pass only protocol identity (e.g., GroupId, recipient_pubkey), never relay URLs. NIP-01 is the base event/relay/auth protocol and is distinct from kind:1 text notes; nmp-nip01 should not be a kind:1-specific crate. Typed action inputs structurally foreclose app involvement in relay selection: PostChatMessageInput contains {group, content, ...} with no relay field, and SendDmInput contains {recipient_pubkey, content, reply_to} with no relay field. ActionModule::execute() returns Result<(), String> and has no return channel for routing; it receives a send: &dyn Fn(ActorCommand) closure and bakes the relay target into the ActorCommand it emits. No new seam is required for relay routing ownership; the two existing seams (PublishUnsignedEventToRelays for synchronous pins, ProtocolCommand for actor-thread resolution) cover both timing cases. Default publish routing follows NIP-65 outbox (D3); PublishTarget::Explicit is the opt-out for cases like gift-wraps that must go to a recipient's inbox relay, bypassing the NIP-65 outbox resolver and sending to exactly the specified relays.

NIP-17 (nmp-nip17) owns gift-wrap routing — the app never selects relays for DMs; the crate supplies PublishTarget::Explicit with the recipient's inbox relay. When the relay set is known synchronously from action input, the NIP crate emits ActorCommand::PublishUnsignedEventToRelays with an explicit relay pin (e.g., NIP-29 reads group.host_relay_url from GroupId). When the relay set is resolved only on the actor thread (requiring cache or signer), the NIP crate emits an ActorCommand::Protocol(ProtocolCommand) that resolves relays via ProtocolCommandContext and emits PublishSignedEvent with PublishTarget::Explicit (e.g., NIP-17 resolves recipient kind:10050 inbox, Marmot resolves group_relay_url).

The kernel enforces fail-closed guard D10: kind:1059 combined with PublishTarget::Auto is refused at the kernel boundary. PublishPlan::validate_no_unpinned_h rejects an h-tagged event with no relay pin, emitting MissingHostPinForGroupEvent. required_dm_relays rejects an empty or missing kind:10050 list rather than falling back to public outbox relays. The generic nmp.publish action (PublishModule) allows the app to pass a PublishTarget as a deliberate D3 opt-out for custom/app-defined kinds; kind:1059 with Auto is refused even there.

The podcast-player's NostrRelayCapability pattern bypasses the NMP publish engine entirely: it reads write-capable relay URLs from app.configured_relays_handle(), falls back to hardcoding wss://relay.primal.net, serializes its own NostrRelayRequest::Publish, and dispatches via raw capability call. The correct fix is to use dispatch_action("nmp.publish", ...) with PublishTarget::Auto for standard events and let the NIP crate own routing for protocol-specific kinds.

<!-- citations: [^13382-1] [^13382-2] [^13382-3] [^13382-4] [^7f143-7] [^cf071-7] -->
