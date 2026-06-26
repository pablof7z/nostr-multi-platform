//! Host-facing runtime events emitted by `BrowserRuntime::pump()`.
//!
//! These mirror the relevant `nmp-wasm` `WorkerEvent` variants but live in the
//! browser-runtime crate so the runtime can surface sign requests and command
//! failures without depending on the ABI-glue crate (epic #2045: nmp-wasm is
//! ABI-only, the browser runtime owns platform behaviour).
//!
//! The host (the wasm Worker bridge, #2048) translates these into the wire
//! `WorkerEvent`s it ships to the main thread.

/// An event produced while draining the inbox or relay queue in `pump()`.
///
/// `Applied` commands produce no event here (their effect is the returned
/// outbound frames + the next snapshot). Only the outcomes that need
/// main-thread cooperation (`NeedsSign`) or that must be surfaced as honest
/// failures (`Unsupported`, relay budget exceeded) emit an event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserRuntimeEvent {
    /// A publish command requires an async sign round-trip (NIP-07 / NIP-46).
    ///
    /// The runtime has already parked the pending publish keyed on
    /// `correlation_id` (see [`crate::BrowserRuntimeHandle::pending_sign_count`]).
    /// The host must broker the signature with the active signer provider and
    /// deliver it back. The signer-provider brokering itself lands in #2049;
    /// this event + the parking are the runtime-side contract that exists now.
    SignRequest {
        /// Correlation id the host echoes back when delivering the signature.
        correlation_id: String,
        /// Hex pubkey of the account expected to sign.
        account_pubkey: String,
        /// The canonical unsigned-event JSON to be signed.
        unsigned_json: String,
    },
    /// A command could not be applied by the headless runtime (it requires the
    /// native actor thread, or no active account was available). Surfaced so the
    /// host never silently drops a command (D6-honest).
    CommandFailed {
        /// The kernel's discriminant-named reason string (e.g.
        /// `browser_command_unsupported: ActorCommand::Identity(..) ...`).
        reason: String,
    },
    /// A sign round-trip settled but could not be matched to a parked publish,
    /// or the kernel reported an unknown/stale correlation id. Surfaced so a
    /// stranded or duplicate sign delivery is observable rather than silently
    /// dropped (D6-honest). Mirrors `nmp-wasm`'s `WorkerEvent::SignFailed`.
    SignFailed {
        /// The sign correlation id that failed to resolve.
        correlation_id: String,
        /// Human-readable reason (kernel failure string, or stale/duplicate
        /// delivery note for an unknown correlation id).
        reason: String,
    },
    /// A relay socket could not be opened because the concurrent-socket budget
    /// (`MAX_CONCURRENT_SOCKETS = 64`) has been reached (#2070).
    ///
    /// The frame targeting `url` was not sent. The host may close an existing
    /// idle socket and retry (future capability). Never a silent drop — D6.
    RelayBudgetExceeded {
        /// The relay URL for which no driver could be spawned.
        url: String,
    },
    /// A relay driver could not be constructed for `url` (the WebSocket
    /// constructor rejected the URL — bad scheme / illegal characters).
    ///
    /// Surfaced on both bootstrap spawn and outbound spawn-on-miss so a frame
    /// targeting an unspawnable relay is never silently dropped (D6-honest).
    RelaySpawnFailed {
        /// The relay URL whose driver could not be constructed.
        url: String,
        /// The stringified constructor error.
        reason: String,
    },
    /// An outbound frame send to an existing driver for `url` failed (the
    /// `WebSocket.send` call threw). The frame did not leave the runtime.
    ///
    /// Surfaced so a failed send is observable rather than swallowed (D6).
    RelaySendFailed {
        /// The relay URL whose driver rejected the send.
        url: String,
        /// The stringified send error.
        reason: String,
    },
    /// One or more inbound relay frames were dropped because the bounded inbound
    /// queue (`MAX_INBOUND_QUEUED = 1024`) overflowed since the previous pump.
    ///
    /// `count` is the number of frames dropped (oldest-first) in that window.
    /// Surfaced so inbound loss is observable rather than silent (D6-honest).
    RelayInboundDropped {
        /// Number of inbound frames dropped since the last pump turn.
        count: u64,
    },
}
