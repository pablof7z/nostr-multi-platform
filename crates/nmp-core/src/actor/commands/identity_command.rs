//! `IdentityCommand` — signer roster + account lifecycle + remote-signer
//! health (ADR-0071).
//!
//! Grouped under `ActorCommand::Identity(IdentityCommand)`. Dispatch home:
//! `actor/dispatch/cmd_identity.rs`.

use std::{collections::HashMap, fmt};

use super::super::SignerSource;

/// Signer-roster management + account lifecycle + remote-signer health slots.
///
/// These are *roster mutations* — they change which signers the actor knows
/// about and which one is active. The ADR-0072 capability-port *verbs* (sign,
/// nip44_encrypt, nip44_decrypt) live in [`super::super::SignCommand`] — they
/// *use* the roster rather than mutating it.
pub enum IdentityCommand {
    /// Unified sign-in command. Adds a signer to the actor-local identity
    /// store from one of the [`SignerSource`] variants and, when `make_active`
    /// is set, binds it as the active signer + retargets the timeline.
    ///
    /// Replaces the old `SignInNsec` / `SignInBunker` / `AddRemoteSigner`
    /// split. D0: the `RemoteHandle` arm's `Box<dyn RemoteSignerHandle>`
    /// concrete type lives in `nmp-signers`; `nmp-core` sees only the trait
    /// object.
    // `allow(dead_code)`: live cross-crate constructors in nmp-ffi — per-crate
    // lint false positive for enum variants constructed outside nmp-core.
    #[allow(dead_code)]
    AddSigner {
        source: SignerSource,
        make_active: bool,
    },
    /// Create a new keypair, publish a kind:0 metadata event and a kind:10002
    /// relay-list event, then register the identity and make it active.
    ///
    /// `profile` is JSON-serialised into the kind:0 `content`. `relays` is a
    /// list of `(url, role)` tuples (`read` | `write` | `both` | `indexer` or a
    /// comma-separated composite). `mls` requests account-scoped MLS setup in
    /// app composition crates. `initial_follows` is the app-supplied hex-pubkey
    /// set the fresh account auto-follows (NMP no longer hardcodes any default
    /// follow set — operator policy originates in the leaf app, #1493).
    CreateAccount {
        profile: HashMap<String, String>,
        relays: Vec<(String, String)>,
        initial_follows: Vec<String>,
        mls: bool,
        /// `true` for the standard onboarding flow; `false` for creating an
        /// agent/secondary account without disturbing the active session.
        make_active: bool,
    },
    /// T66a identity — switch the active account (synchronous re-bind +
    /// timeline retarget, mirrors `AccountManager::switch_active` semantics).
    SwitchActive { identity_id: String },
    /// T66a identity — remove an account; clears the active slot if it was
    /// the active one.
    RemoveAccount { identity_id: String },
    /// Broker adapter → actor: progress event for the bunker handshake UI.
    /// Actor stores the latest into a kernel snapshot field; the adapter is
    /// the sole writer. Stage `"idle"` clears the projection.
    #[allow(dead_code)] // live cross-crate caller in nmp-ffi — per-crate lint false positive
    BunkerHandshakeProgress {
        /// `"connecting"` | `"awaiting_pubkey"` | `"ready"` | `"failed"` | `"idle"`.
        stage: String,
        /// Stable machine code for a user-facing progress label
        /// (`nmp_nip46::progress_codes::*`); `None` for diagnostic /
        /// `"failed"` transitions. The shell localizes the code, falling back
        /// to `message` (#1711).
        code: Option<String>,
        /// Optional human-readable status (the English fallback prose / error
        /// reason).
        message: Option<String>,
    },
    /// V-14 step b — relay-layer connection state update for the NIP-46 bunker
    /// session. The actor writes it to the shared `SignerStateSlot` (ADR-0072
    /// D6 — the unified remote-signer health slot); the built-in
    /// `"signer_state"` snapshot projection reads the slot on every tick. D4:
    /// the actor is the sole writer of the slot — the broker callback routes
    /// through this command so the write happens on the actor thread.
    #[allow(dead_code)] // live cross-crate caller in nmp-ffi — per-crate lint false positive
    BunkerConnectionStateChanged {
        /// `"connected"` | `"reconnecting"` | `"failed"`.
        state: String,
        /// Optional human-readable reason (error message on reconnect/failed).
        reason: Option<String>,
    },
    /// ADR-0072 D6 — NIP-55 external-signer health update for the unified
    /// `signer_state` projection. Emitted by the `nmp-ffi` NIP-55 driver when
    /// the host capability bridge reports an outcome that affects long-lived
    /// signer health (awaiting approval, ready, rejected, unavailable).
    #[allow(dead_code)] // live cross-crate caller in nmp-ffi — per-crate lint false positive
    Nip55SignerStateChanged {
        /// `"ready"` | `"awaiting_approval"` | `"unavailable"` | `"failed"`.
        state: String,
        /// Optional human-readable reason (rejection/unavailable detail).
        reason: Option<String>,
    },
    /// Deliver an inbound remote-signer response for correlation-keyed
    /// dispatch (ADR-0072 §D3b) — the actor-mailbox completion path for
    /// steady-state replies. Both backends route here instead of resolving
    /// the parked op on a foreign thread: NIP-46 via the broker's opaque
    /// completion sink, NIP-55 via `external_signer.rs::deliver`. The arm
    /// fans the JSON to every remote handle (each drops non-matching ids —
    /// the trait contract). Because the send lands on the single waking
    /// inbox (§D3a), the completion wakes the actor and the SAME iteration
    /// drains the parked-op queue — no ≤250ms tick dependence; the
    /// pending-map mutation is on the actor thread (D4 single-writer).
    DeliverSignerResponse {
        /// The already-decoded signer response (NIP-46: decrypted RPC body;
        /// NIP-55: serialized `ExternalSignerResponse`), passed verbatim to
        /// each handle's `deliver_response`.
        response_json: String,
    },
    /// ADR-0072 §3 — re-entry command from the serialized capability-worker
    /// thread (V-90 Site 2). The worker runs `dispatch_capability` off the
    /// actor thread and posts this command with the result; the actor
    /// applies it inside a normal tick (D4 single-writer invariant).
    ///
    /// The `account_id` field carries the originating account so the dispatch
    /// arm can verify the account still exists before applying — a result for
    /// a since-removed account is dropped with a D6 trace (never cross-applied
    /// to the now-active account).
    #[cfg(feature = "native")]
    CapabilityResultReady {
        /// Originating account id (the `account_id` field from the keyring
        /// request). Used solely for the removed-account guard.
        account_id: String,
        /// `CapabilityEnvelope` JSON returned by the native handler. The
        /// dispatch arm decodes it and emits an error toast when `status` is
        /// not `"ok"` and the account is still present.
        result_json: String,
    },
}

impl fmt::Debug for IdentityCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AddSigner {
                source,
                make_active,
            } => f
                .debug_struct("AddSigner")
                .field("source", source)
                .field("make_active", make_active)
                .finish(),
            Self::CreateAccount {
                profile,
                relays,
                initial_follows,
                mls,
                make_active,
            } => f
                .debug_struct("CreateAccount")
                .field("profile", profile)
                .field("relays", relays)
                .field("initial_follows", initial_follows)
                .field("mls", mls)
                .field("make_active", make_active)
                .finish(),
            Self::SwitchActive { identity_id } => f
                .debug_struct("SwitchActive")
                .field("identity_id", identity_id)
                .finish(),
            Self::RemoveAccount { identity_id } => f
                .debug_struct("RemoveAccount")
                .field("identity_id", identity_id)
                .finish(),
            Self::BunkerHandshakeProgress {
                stage,
                code,
                message,
            } => f
                .debug_struct("BunkerHandshakeProgress")
                .field("stage", stage)
                .field("code", code)
                .field("message", message)
                .finish(),
            Self::BunkerConnectionStateChanged { state, reason } => f
                .debug_struct("BunkerConnectionStateChanged")
                .field("state", state)
                .field("reason", reason)
                .finish(),
            Self::Nip55SignerStateChanged { state, reason } => f
                .debug_struct("Nip55SignerStateChanged")
                .field("state", state)
                .field("reason", reason)
                .finish(),
            Self::DeliverSignerResponse { .. } => f
                .debug_struct("DeliverSignerResponse")
                .field("response_json", &"[redacted]")
                .finish(),
            #[cfg(feature = "native")]
            Self::CapabilityResultReady { account_id, .. } => f
                .debug_struct("CapabilityResultReady")
                .field("account_id", account_id)
                .field("result_json", &"[redacted]")
                .finish(),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::actor::{ActorCommand, ActorMail};

    use super::IdentityCommand;

    #[test]
    fn deliver_signer_response_debug_redacts_response_json() {
        let cmd = IdentityCommand::DeliverSignerResponse {
            response_json: r#"{"id":"rpc-1","result":{"session_id":"session-secret","items":[{"plaintext":"plain-secret"}]}}"#.to_string(),
        };

        let s = format!("{cmd:?}");
        assert!(s.contains("DeliverSignerResponse"));
        assert!(!s.contains("session-secret"));
        assert!(!s.contains("plain-secret"));
        assert!(s.contains("[redacted]"));
    }

    #[test]
    fn actor_mail_debug_redacts_deliver_signer_response_json() {
        let mail = ActorMail::Command(ActorCommand::Identity(
            IdentityCommand::DeliverSignerResponse {
                response_json: r#"{"id":"rpc-1","result":"plain-secret"}"#.to_string(),
            },
        ));

        let s = format!("{mail:?}");
        assert!(s.contains("DeliverSignerResponse"));
        assert!(!s.contains("plain-secret"));
        assert!(s.contains("[redacted]"));
    }
}
