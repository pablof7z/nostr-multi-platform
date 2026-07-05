//! #3010 — `SendNutzap` parked awaiting the recipient's kind:10019 (NIP-61
//! nutzap info) instead of failing closed on first cache miss. Split out of
//! `state.rs` (AGENTS.md LOC discipline). See `backend::cashu::nutzap_await`'s
//! module docs for the full event-driven continuation this state drives: the
//! `IngestParser` that redrives a parked entry the instant a matching
//! kind:10019 arrives, and the `RelayTextInterceptor` that bounds the wait.

use super::CashuWalletState;

/// One `SendNutzap` waiting on `recipient_pubkey`'s kind:10019, keyed by that
/// pubkey in [`CashuWalletState::pending_sends`]. The ORIGINAL `SendNutzap`
/// operation is already transitioned `Failed` (superseded, journal-only — no
/// `ShowErrorToken`/`RecordActionFailure` sent yet) by the time this is
/// parked (see `send.rs`'s miss branch), exactly mirroring
/// `cross_mint_worker::SendRetry`'s "supersede this attempt, resolve the
/// caller's correlation id on a fresh redrive instead" shape.
#[derive(Clone)]
pub(in crate::backend::cashu) struct PendingSendAwait {
    /// A monotonic, per-process-lifetime id assigned at park time
    /// ([`CashuWalletState::park_send_await`]) — never reused, so the
    /// eventual redrive's fresh journal operation id
    /// (`nutzap-await-redrive-{await_id}`) can never collide with another
    /// parked await or with the original (superseded) send's own operation
    /// id.
    pub(in crate::backend::cashu) await_id: u64,
    pub(in crate::backend::cashu) account_pubkey: String,
    pub(in crate::backend::cashu) recipient_pubkey: String,
    pub(in crate::backend::cashu) amount_sats: u64,
    pub(in crate::backend::cashu) target_event_id: Option<String>,
    pub(in crate::backend::cashu) correlation_id: Option<String>,
    /// Wall-clock seconds this await was parked — the bound
    /// (`nutzap_await::NUTZAP_INFO_AWAIT_TIMEOUT_SECS`) is measured from
    /// here, swept by [`CashuWalletState::sweep_expired_send_awaits`].
    pub(in crate::backend::cashu) parked_at_secs: u64,
}

impl CashuWalletState {
    /// Park a `SendNutzap` awaiting `recipient_pubkey`'s kind:10019, keyed
    /// under that recipient in `pending_sends`. Returns the freshly assigned
    /// [`PendingSendAwait::await_id`] (unused by the caller today, but kept
    /// so a future caller — or a test — can trace a specific parked entry).
    pub(in crate::backend::cashu) fn park_send_await(
        &mut self,
        recipient_pubkey: &str,
        account_pubkey: String,
        amount_sats: u64,
        target_event_id: Option<String>,
        correlation_id: Option<String>,
        now_secs: u64,
    ) -> u64 {
        let await_id = self.next_send_await_id;
        self.next_send_await_id += 1;
        self.pending_sends
            .entry(recipient_pubkey.to_string())
            .or_default()
            .push(PendingSendAwait {
                await_id,
                account_pubkey,
                recipient_pubkey: recipient_pubkey.to_string(),
                amount_sats,
                target_event_id,
                correlation_id,
                parked_at_secs: now_secs,
            });
        await_id
    }

    /// Remove and return every `SendNutzap` parked on `recipient_pubkey` —
    /// the at-most-once chokepoint: once taken, an entry can never be handed
    /// out again (a duplicate re-delivery of the same kind:10019, from a
    /// second relay or a re-observed EOSE replay, finds nothing left to
    /// redrive).
    pub(in crate::backend::cashu) fn take_send_awaits(
        &mut self,
        recipient_pubkey: &str,
    ) -> Vec<PendingSendAwait> {
        self.pending_sends
            .remove(recipient_pubkey)
            .unwrap_or_default()
    }

    /// Remove and return every parked await, across every recipient, whose
    /// age (`now_secs - parked_at_secs`) has reached `timeout_secs` — the
    /// bounded-fallback half of #3010 (a genuinely-absent recipient must
    /// still terminate). Entries not yet expired are left in place.
    pub(in crate::backend::cashu) fn sweep_expired_send_awaits(
        &mut self,
        now_secs: u64,
        timeout_secs: u64,
    ) -> Vec<PendingSendAwait> {
        let mut expired = Vec::new();
        let mut empties = Vec::new();
        for (recipient, awaits) in self.pending_sends.iter_mut() {
            let mut i = 0;
            while i < awaits.len() {
                if now_secs.saturating_sub(awaits[i].parked_at_secs) >= timeout_secs {
                    expired.push(awaits.remove(i));
                } else {
                    i += 1;
                }
            }
            if awaits.is_empty() {
                empties.push(recipient.clone());
            }
        }
        for recipient in empties {
            self.pending_sends.remove(&recipient);
        }
        expired
    }
}

#[cfg(test)]
mod tests {
    use super::CashuWalletState;

    fn park(state: &mut CashuWalletState, recipient: &str, now: u64) -> u64 {
        state.park_send_await(recipient, "acct".to_string(), 21, None, None, now)
    }

    #[test]
    fn park_assigns_monotonically_increasing_ids() {
        let mut state = CashuWalletState::new();
        let a = park(&mut state, "recipient-a", 100);
        let b = park(&mut state, "recipient-a", 100);
        let c = park(&mut state, "recipient-b", 100);
        assert!(a < b && b < c);
    }

    #[test]
    fn take_removes_only_the_matching_recipients_awaits() {
        let mut state = CashuWalletState::new();
        park(&mut state, "recipient-a", 100);
        park(&mut state, "recipient-b", 100);
        let taken = state.take_send_awaits("recipient-a");
        assert_eq!(taken.len(), 1);
        assert_eq!(taken[0].recipient_pubkey, "recipient-a");
        assert!(state.take_send_awaits("recipient-a").is_empty());
        assert_eq!(state.take_send_awaits("recipient-b").len(), 1);
    }

    #[test]
    fn take_is_at_most_once_a_second_take_finds_nothing() {
        let mut state = CashuWalletState::new();
        park(&mut state, "recipient-a", 100);
        assert_eq!(state.take_send_awaits("recipient-a").len(), 1);
        // A duplicate delivery of the same kind:10019 (second relay, or a
        // re-observed EOSE replay) must never redrive a second time.
        assert!(state.take_send_awaits("recipient-a").is_empty());
    }

    #[test]
    fn sweep_only_removes_entries_past_the_timeout() {
        let mut state = CashuWalletState::new();
        park(&mut state, "recipient-a", 100); // parked at t=100
        park(&mut state, "recipient-b", 190); // parked at t=190
                                              // now=200, timeout=20 -> recipient-a (age 100) expired,
                                              // recipient-b (age 10) not yet.
        let expired = state.sweep_expired_send_awaits(200, 20);
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].recipient_pubkey, "recipient-a");
        assert!(state.take_send_awaits("recipient-a").is_empty());
        assert_eq!(state.take_send_awaits("recipient-b").len(), 1);
    }

    #[test]
    fn sweep_removes_multiple_expired_awaits_for_the_same_recipient() {
        let mut state = CashuWalletState::new();
        park(&mut state, "recipient-a", 100);
        park(&mut state, "recipient-a", 100);
        let expired = state.sweep_expired_send_awaits(200, 20);
        assert_eq!(expired.len(), 2);
        assert!(state.take_send_awaits("recipient-a").is_empty());
    }
}
