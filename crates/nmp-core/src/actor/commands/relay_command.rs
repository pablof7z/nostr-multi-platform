//! `RelayCommand` — relay-list edits + transport-layer control (ADR-0065).
//!
//! Grouped under `ActorCommand::Relay(RelayCommand)`. Dispatch home:
//! `actor/dispatch/cmd_publish.rs` (relay-mutation path).

/// Relay-list edits + transport-layer control verbs.
///
/// Each variant mutates the user's kind:10002 relay list (or drives the
/// transport layer directly). The dispatch arm routes through
/// `cmd_publish::add_relay` / `cmd_publish::remove_relay` /
/// `cmd_publish::reconnect_relays_cmd` / `cmd_publish::set_relay_info`.
#[derive(Debug)]
pub enum RelayCommand {
    /// T66a relay edit — add a relay row (role: `read` | `write` | `both`).
    AddRelay {
        url: String,
        role: String,
    },
    /// T66a relay edit — remove a relay row.
    RemoveRelay {
        url: String,
    },
    /// Kernel-side "reconnect all" (#1689): re-dial every disconnected/errored
    /// relay worker in the pool. Host apps drive it after a network change /
    /// app-foreground so the kernel stays the sole driver of transport. See
    /// [`super::super::super::relay_reconnect::reconnect_relays`] for the
    /// idempotency and fail-closed contract.
    ReconnectRelays,
    /// Store a fetched relay-information document on the kernel's per-URL
    /// transport row (ADR-0051). Posted by the `nmp-nip11` fetch worker; the
    /// dispatch arm folds the parsed `RelayInfoDoc` via
    /// [`Kernel::set_relay_info`] so the `relay_diagnostics` projection
    /// surfaces it. `nmp-core` names no NIP-11 noun — it carries the
    /// substrate-generic `RelayInfoDoc` (D0); malformed JSON is a no-op (D6).
    SetRelayInfo {
        /// The relay URL the document was fetched for (canonicalised on store).
        relay_url: String,
        /// `RelayInfoDoc` serialised via `RelayInfoDoc::to_json`.
        doc_json: String,
    },
}