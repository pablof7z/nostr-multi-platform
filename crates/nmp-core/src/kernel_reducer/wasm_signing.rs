//! #1753 S6 — the wasm signing capability round-trip on `KernelReducer`
//! (ADR-0050 §D1/§D3b, pure message re-entry, NO polling — D8).
//!
//! # The round-trip
//!
//! A web host cannot sign on the worker (Web Workers have no `window`, so no
//! `window.nostr`). NIP-07 signing is therefore a **capability round-trip**
//! brokered by the **main thread**:
//!
//! 1. The reducer (running on the worker, or wherever `KernelReducer` lives)
//!    receives a sign request and calls [`KernelReducer::begin_sign_roundtrip_at`].
//!    That **parks a sign op** in the shared [`ParkedSignerOps`] queue — the
//!    SAME component the native actor loop owns — pins it to the active account,
//!    and returns a [`SignRoundTripRequest`] the host posts to the main thread.
//! 2. The main-thread JS bridge plays the broker role: it calls
//!    `window.nostr.signEvent(...)` and posts the signed flat-NIP-01 JSON back
//!    to the reducer as a `DeliverSignerResponse`-shaped message.
//! 3. The reducer calls [`KernelReducer::deliver_signed_response`]. That hands
//!    the signed value to the parked op and **drives the queue exactly once,
//!    from inside this message handler** — pure message re-entry. The parked
//!    op's continuation runs, records the completion, and the op is dropped.
//!
//! # D8 — no polling, no tick-dependence, no blocking recv
//!
//! There is NO timer, NO poll loop, and NO [`SignerOp::wait`] (blocking recv)
//! anywhere in this path. The parked op is resolved because the inbound
//! `deliver_signed_response` message arrived and drove the queue once — not
//! because anything polled for it. This is the *same mechanism* the native
//! NIP-46 broker uses (the broker resolves the op's channel out of band; the
//! drain picks the value up with a single non-blocking `poll`), reusing the ONE
//! `ParkedSignerOps::drive_at` driver. The only difference is the drive trigger:
//! the native idle tick vs. the wasm completion message. The
//! [`crate::kernel_reducer::wasm_signing::no_polling_oracle_tests`] module
//! proves completion happens via message re-entry rather than any poll/sleep.
//!
//! # Account-pinning
//!
//! [`begin_sign_roundtrip_at`] records the active account pubkey at park time. The
//! continuation cross-checks the signed event's author against that pin and
//! rejects a mismatch — a mid-flight account switch cannot deliver a signature
//! from a different key into the originating request (ADR-0050 §D5 pinning).
//!
//! # Round-trip completion and publish
//!
//! S6 wires the *signing round-trip mechanism only*. The completion continuation
//! records the signed event into an observable sink. After #1008 the wasm runtime
//! handles `PublishRawEvent` / `PublishProfile` actor commands by calling
//! `begin_sign_roundtrip_at`, and the signed result is delivered here — the host
//! then dispatches a `PublishSignedEvent` command (or the future S6b continuation
//! publishes inline). The host reads the `SignRoundTripOutcome` to confirm the
//! signing step completed.

use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

// D20: the wasm signing round-trip runs on the wasm path, so the park deadline
// type must come from the `crate::time` shim (re-exports `std::time` on native,
// `web_time` on wasm32).
use crate::time::Instant;

use nmp_signer_iface::{SignerError, SignerOp};

use crate::actor::pending_sign::{ParkedOp, ParkedSignerOps};
use crate::actor::SignContinuation;
use nmp_signer_iface::{SignedEvent, UnsignedEvent};

/// The op-completion budget for a wasm NIP-07 sign round-trip. The user must
/// approve in the extension UI, which can take a few seconds; this is the
/// wall-clock deadline the drive's timeout gate enforces (D8). Generous because
/// the only cost of a too-short deadline is a spurious `Timeout` terminal, and
/// the only cost of a too-long one is a stale parked op occupying a slot until
/// the next drive — neither blocks anything.
const WASM_SIGN_OP_TIMEOUT: Duration = Duration::from_secs(60);

/// Shared, `Send`-able completion sink. `SignContinuation`'s closure is
/// `Send + 'static`, so the sink it pushes into must be `Arc<Mutex<..>>` even on
/// single-threaded wasm.
type SharedCompletions = Arc<Mutex<Vec<SignRoundTripCompletion>>>;

/// Per-`KernelReducer` wasm-signing state. Defaults to empty; only the
/// `begin_sign_roundtrip_at` / `deliver_signed_response_at` seam touches it. Native
/// reducers never call that seam, so this stays inert there.
pub(crate) struct SignRoundTripState {
    /// The shared parked-op queue + drain driver (#1753). One `drive` call per
    /// `deliver_signed_response` message — never on a timer (D8).
    parked: ParkedSignerOps,
    /// Per-correlation value-delivery senders. `deliver_signed_response` removes
    /// the matching sender and sends the parsed `SignedEvent` (or `Err`) on it,
    /// then drives the queue once. The sender resolving the op's channel is the
    /// out-of-band delivery; the `drive`'s single `poll` picks it up.
    senders: HashMap<String, mpsc::Sender<Result<SignedEvent, SignerError>>>,
    /// Observable completion sink. The continuation pushes the outcome here so a
    /// host (and the no-polling oracle test) can confirm the round-trip
    /// completed. After #1008 the wasm runtime publishes via a follow-up
    /// `PublishSignedEvent` dispatch after reading this completion.
    completions: SharedCompletions,
}

impl Default for SignRoundTripState {
    fn default() -> Self {
        Self {
            parked: ParkedSignerOps::new(),
            senders: HashMap::new(),
            completions: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

/// The request a host posts to the main-thread broker after
/// [`KernelReducer::begin_sign_roundtrip_at`]. The broker calls
/// `window.nostr.signEvent(unsigned)` and posts the result back keyed on
/// `correlation_id`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignRoundTripRequest {
    /// The correlation id the broker must echo in its `deliver_signed_response`.
    pub correlation_id: String,
    /// The account this sign is pinned to (lowercase hex). The broker should
    /// ensure `window.nostr` is on this account before signing.
    pub account_pubkey: String,
    /// The unsigned flat-NIP-01 event JSON to hand to `window.nostr.signEvent`.
    pub unsigned_json: String,
}

/// The outcome of a [`KernelReducer::deliver_signed_response`] call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SignRoundTripOutcome {
    /// The signed event matched a parked request and resolved it this call (the
    /// continuation ran). Carries the signed flat-NIP-01 JSON the host may log.
    Completed { correlation_id: String, signed_json: String },
    /// The delivered response failed the round-trip (parse error, account-pin
    /// mismatch, or signer-reported error). The parked op is resolved with the
    /// error terminal so nothing is left dangling (D6).
    Failed { correlation_id: String, reason: String },
    /// No parked request matched the correlation id (a stale / duplicate
    /// delivery). A no-op — nothing was parked, nothing resolved.
    Unknown { correlation_id: String },
}

/// One recorded round-trip completion, observable through
/// [`KernelReducer::take_sign_completions`]. Behind the honest-disable gate the
/// signed event is recorded, NOT published.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignRoundTripCompletion {
    pub correlation_id: String,
    pub outcome: Result<String, String>,
}

/// Parse a flat-NIP-01 signed event JSON (`{id,pubkey,created_at,kind,tags,
/// content,sig}`) into a [`SignedEvent`]. Total: returns `Err(reason)` on any
/// shape mismatch (D6) — never panics.
fn parse_signed_flat_json(signed_json: &str) -> Result<SignedEvent, String> {
    #[derive(serde::Deserialize)]
    struct Flat {
        id: String,
        pubkey: String,
        kind: u32,
        tags: Vec<Vec<String>>,
        content: String,
        created_at: u64,
        sig: String,
    }
    let flat: Flat = serde_json::from_str(signed_json)
        .map_err(|e| format!("signed event JSON did not parse: {e}"))?;
    Ok(SignedEvent {
        id: flat.id,
        sig: flat.sig,
        unsigned: UnsignedEvent {
            pubkey: flat.pubkey,
            kind: flat.kind,
            tags: flat.tags,
            content: flat.content,
            created_at: flat.created_at,
        },
    })
}

/// Parse an unsigned flat-NIP-01 event JSON into an [`UnsignedEvent`] for the
/// host bridge to hand to `window.nostr.signEvent`. Accepts either the flat
/// wire shape (with a top-level `pubkey`) or this crate's nested `UnsignedEvent`
/// serde shape, so a host can post whichever it has.
fn parse_unsigned_flat_json(unsigned_json: &str) -> Result<UnsignedEvent, String> {
    // Try the nested `UnsignedEvent` derive shape first (what `serde_json` of an
    // `UnsignedEvent` produces), then fall back to the flat wire shape.
    if let Ok(u) = serde_json::from_str::<UnsignedEvent>(unsigned_json) {
        return Ok(u);
    }
    #[derive(serde::Deserialize)]
    struct Flat {
        pubkey: String,
        kind: u32,
        tags: Vec<Vec<String>>,
        content: String,
        created_at: u64,
    }
    let flat: Flat = serde_json::from_str(unsigned_json)
        .map_err(|e| format!("unsigned event JSON did not parse: {e}"))?;
    Ok(UnsignedEvent {
        pubkey: flat.pubkey,
        kind: flat.kind,
        tags: flat.tags,
        content: flat.content,
        created_at: flat.created_at,
    })
}

impl super::KernelReducer {
    /// #1753 S6 — begin a NIP-07 sign capability round-trip. Parks a sign op in
    /// the shared [`ParkedSignerOps`] queue, pins it to `account_pubkey`, and
    /// returns the [`SignRoundTripRequest`] the host posts to the main-thread
    /// broker. The op resolves later via [`Self::deliver_signed_response`] —
    /// pure message re-entry, no polling (D8).
    ///
    /// `account_pubkey` is the lowercase-hex account the sign is bound to (the
    /// host already knows it from the NIP-07 `getPublicKey()` handshake). The
    /// `unsigned_json` is the flat-NIP-01 (or nested-`UnsignedEvent`) JSON to be
    /// signed; it is re-serialized into the request's `unsigned_json` in the
    /// canonical flat wire shape.
    ///
    /// Total (D6): a malformed `unsigned_json` returns `Err(reason)` and parks
    /// nothing.
    pub fn begin_sign_roundtrip_at(
        &mut self,
        account_pubkey: String,
        unsigned_json: &str,
        now: Instant,
    ) -> Result<SignRoundTripRequest, String> {
        let unsigned = parse_unsigned_flat_json(unsigned_json)?;
        // Re-serialize to the canonical flat wire shape the JS bridge expects.
        let flat_unsigned_json = serde_json::json!({
            "pubkey": unsigned.pubkey,
            "kind": unsigned.kind,
            "tags": unsigned.tags,
            "content": unsigned.content,
            "created_at": unsigned.created_at,
        })
        .to_string();

        let correlation_id = next_correlation_id(&account_pubkey, &unsigned);

        // The value channel that `deliver_signed_response` resolves out of band;
        // the parked op's `SignerOp::Pending(rx)` is polled exactly once when the
        // queue is driven from that message handler (no blocking recv — D8).
        let (tx, rx) = mpsc::channel::<Result<SignedEvent, SignerError>>();

        let pin = account_pubkey.clone();
        let cid_for_continuation = correlation_id.clone();
        let completions = std::sync::Arc::clone(&self.sign_roundtrip.completions);
        let continuation = SignContinuation::new(move |outcome| {
            // Account-pinning enforcement: verify author matches the request's
            // pinned pubkey (ADR-0050 §D5), then record the signed event JSON
            // in the completion sink for the host to dispatch `PublishSignedEvent`.
            let recorded = match outcome {
                Ok(signed) => {
                    if signed.unsigned.pubkey != pin {
                        Err(format!(
                            "account-pin mismatch: request pinned to {pin}, signature \
                             authored by {} — refusing to cross-deliver (ADR-0050 §D5)",
                            signed.unsigned.pubkey
                        ))
                    } else {
                        Ok(signed.to_nip01_json())
                    }
                }
                Err(reason) => Err(reason),
            };
            if let Ok(mut sink) = completions.lock() {
                sink.push(SignRoundTripCompletion {
                    correlation_id: cid_for_continuation.clone(),
                    outcome: recorded,
                });
            }
        });

        let deadline = now + WASM_SIGN_OP_TIMEOUT;
        self.sign_roundtrip
            .parked
            .push(ParkedOp::sign_continuation(SignerOp::Pending(rx), continuation, deadline));
        self.sign_roundtrip
            .senders
            .insert(correlation_id.clone(), tx);

        Ok(SignRoundTripRequest {
            correlation_id,
            account_pubkey,
            unsigned_json: flat_unsigned_json,
        })
    }

    /// Test-support convenience wrapper. Production callers pass the message
    /// boundary timestamp explicitly through [`Self::begin_sign_roundtrip_at`].
    #[cfg(any(test, feature = "test-support"))]
    pub fn begin_sign_roundtrip(
        &mut self,
        account_pubkey: String,
        unsigned_json: &str,
    ) -> Result<SignRoundTripRequest, String> {
        self.begin_sign_roundtrip_at(
            account_pubkey,
            unsigned_json,
            crate::kernel::test_support::test_support_now(),
        )
    }

    /// #1753 S6 — deliver a signed (or failed) response for a parked sign
    /// round-trip. THIS is the message re-entry: it hands the value to the
    /// parked op and **drives the shared queue exactly once, here, from the
    /// inbound message** — no polling, no timer, no blocking recv (D8).
    ///
    /// `signed_json` is the flat-NIP-01 signed event the main-thread broker got
    /// back from `window.nostr.signEvent`. Pass an `Err` shape by supplying a
    /// non-event string only if you intend a parse failure; signer-side errors
    /// should be delivered through [`Self::fail_sign_roundtrip_at`] instead.
    ///
    /// Total (D6): an unknown correlation id is a no-op
    /// ([`SignRoundTripOutcome::Unknown`]); a malformed `signed_json` resolves
    /// the parked op with an error terminal.
    pub fn deliver_signed_response_at(
        &mut self,
        correlation_id: &str,
        signed_json: &str,
        now: Instant,
    ) -> SignRoundTripOutcome {
        let Some(tx) = self.sign_roundtrip.senders.remove(correlation_id) else {
            return SignRoundTripOutcome::Unknown {
                correlation_id: correlation_id.to_string(),
            };
        };

        // Resolve the op's channel out of band (the broker delivery), then drive
        // the queue ONCE from this message handler. The drive's single `poll`
        // picks up the value — message re-entry, not a poll loop.
        match parse_signed_flat_json(signed_json) {
            Ok(signed) => {
                let _ = tx.send(Ok(signed));
            }
            Err(reason) => {
                let _ = tx.send(Err(SignerError::Backend(reason)));
            }
        }
        // Drop our sender clone is unnecessary — `tx` is moved by `send`; the
        // original is the only one. The op's `rx` now has exactly one value.
        self.drive_sign_roundtrip_at(now);

        self.report_completion(correlation_id)
    }

    /// Test-support convenience wrapper. Production callers pass the message
    /// boundary timestamp explicitly through [`Self::deliver_signed_response_at`].
    #[cfg(any(test, feature = "test-support"))]
    pub fn deliver_signed_response(
        &mut self,
        correlation_id: &str,
        signed_json: &str,
    ) -> SignRoundTripOutcome {
        self.deliver_signed_response_at(
            correlation_id,
            signed_json,
            crate::kernel::test_support::test_support_now(),
        )
    }

    /// #1753 S6 — fail a parked sign round-trip (e.g. the user rejected in the
    /// extension, or `window.nostr` was absent). Resolves the parked op with the
    /// supplied error terminal via the same single drive (D6 — nothing left
    /// dangling; D8 — message re-entry, no poll).
    pub fn fail_sign_roundtrip_at(
        &mut self,
        correlation_id: &str,
        reason: &str,
        now: Instant,
    ) -> SignRoundTripOutcome {
        let Some(tx) = self.sign_roundtrip.senders.remove(correlation_id) else {
            return SignRoundTripOutcome::Unknown {
                correlation_id: correlation_id.to_string(),
            };
        };
        let _ = tx.send(Err(SignerError::Backend(reason.to_string())));
        self.drive_sign_roundtrip_at(now);
        self.report_completion(correlation_id)
    }

    /// Test-support convenience wrapper. Production callers pass the message
    /// boundary timestamp explicitly through [`Self::fail_sign_roundtrip_at`].
    #[cfg(any(test, feature = "test-support"))]
    pub fn fail_sign_roundtrip(
        &mut self,
        correlation_id: &str,
        reason: &str,
    ) -> SignRoundTripOutcome {
        self.fail_sign_roundtrip_at(
            correlation_id,
            reason,
            crate::kernel::test_support::test_support_now(),
        )
    }

    /// The ONE drive of the wasm round-trip queue. Called only from a
    /// completion-message handler (`deliver_signed_response_at` /
    /// `fail_sign_roundtrip_at`) — never on a timer, never in a loop (D8).
    fn drive_sign_roundtrip_at(&mut self, now: Instant) {
        // The wasm round-trip parks only `SignContinuation` sinks, which settle
        // in-drain (no `Publish` / `Auth` obligations); the returned batch's
        // obligation vecs are therefore always empty. The actual relay-publish
        // happens via a follow-up `PublishSignedEvent` dispatch from the host
        // after reading the completion sink (#1008).
        let _batch = self.sign_roundtrip.parked.drive_at(&mut self.kernel, now);
    }

    /// Build the [`SignRoundTripOutcome`] for `correlation_id` from the most
    /// recent recorded completion. The continuation always records exactly one
    /// completion per resolved op, so the matching entry is the outcome.
    fn report_completion(&self, correlation_id: &str) -> SignRoundTripOutcome {
        let Ok(sink) = self.sign_roundtrip.completions.lock() else {
            return SignRoundTripOutcome::Failed {
                correlation_id: correlation_id.to_string(),
                reason: "completion sink lock poisoned".to_string(),
            };
        };
        match sink.iter().rev().find(|c| c.correlation_id == correlation_id) {
            Some(c) => match &c.outcome {
                Ok(signed_json) => SignRoundTripOutcome::Completed {
                    correlation_id: correlation_id.to_string(),
                    signed_json: signed_json.clone(),
                },
                Err(reason) => SignRoundTripOutcome::Failed {
                    correlation_id: correlation_id.to_string(),
                    reason: reason.clone(),
                },
            },
            // The op was driven but recorded nothing — only possible if it is
            // still pending (no value delivered). Should not happen on this path
            // (we always send before driving); surface as Unknown rather than
            // claiming a false Completed.
            None => SignRoundTripOutcome::Unknown {
                correlation_id: correlation_id.to_string(),
            },
        }
    }

    /// #1753 S6 — drain and return the recorded round-trip completions. The host
    /// reads these to confirm the signing mechanism worked and dispatches a
    /// follow-up `PublishSignedEvent` to complete the publish (#1008). Drains
    /// so each completion is observed once.
    #[must_use]
    pub fn take_sign_completions(&mut self) -> Vec<SignRoundTripCompletion> {
        match self.sign_roundtrip.completions.lock() {
            Ok(mut sink) => std::mem::take(&mut *sink),
            Err(_) => Vec::new(),
        }
    }

    /// Number of sign round-trips currently parked (awaiting a broker response).
    /// Diagnostics / oracle assertions.
    #[must_use]
    pub fn pending_sign_roundtrips(&self) -> usize {
        self.sign_roundtrip.parked.len()
    }
}

/// A stable, collision-resistant correlation id derived from the account +
/// unsigned event content + a monotonic counter. Deterministic enough for
/// tests, unique enough for concurrent in-flight signs.
fn next_correlation_id(account_pubkey: &str, unsigned: &UnsignedEvent) -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let seq = COUNTER.fetch_add(1, Ordering::Relaxed);
    format!(
        "nip07-sign-{}-{}-{seq}",
        &account_pubkey.get(..8).unwrap_or(account_pubkey),
        unsigned.kind
    )
}

#[cfg(test)]
#[path = "wasm_signing_tests.rs"]
mod wasm_signing_tests;

#[cfg(test)]
#[path = "no_polling_oracle_tests.rs"]
mod no_polling_oracle_tests;
