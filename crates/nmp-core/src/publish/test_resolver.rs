//! `TestKind10002OutboxResolver` — test-only NIP-65 outbox resolver that
//! reads `kind:10002` write-relays from an [`EventStore`] (see
//! `crate::store`).
//!
//! Spec §271 (2026-05-25): the production `Nip65OutboxResolver` lives in
//! `nmp-router`. The dozens of in-tree `#[cfg(test)]` test suites in
//! `nmp-core` (`publish_engine_tests`, `outbox_tests`,
//! `action_failure_tests`, `publish_terminal_status_tests`,
//! `eose_ok_notice_ingest_tests`, `actor::commands::tests`,
//! `actor::commands::remote_signer_tests`, every test that goes through
//! `kernel::test_support::seed_kind10002_for_test`) auto-install this
//! stripped-down resolver via `Kernel::with_optional_publish_store_and_path`
//! → `PublishEngine::set_outbox` instead. Pulling in `nmp-router` as an
//! `nmp-core` dev-dep would create a feature-incompatible dev cycle
//! (`nmp-router` already depends on `nmp-core`; the test-target dev-dep
//! flip activates `test-support` features on `nmp-core` that the
//! production `nmp-router` doesn't see, so the type tables don't unify
//! and Rust reports "multiple different versions of `nmp_core` in the
//! dependency graph").
//!
//! Behaviour mirrors the router-side resolver for the lanes the in-tree
//! tests actually exercise:
//!
//! - explicit targets pass through
//! - for `Auto`: union the author's kind:10002 write entries
//!   (incl. unmarked = both) + the active-account local-write-relays
//!   fallback when the author IS the active account and has no
//!   kind:10002 yet + each recipient's kind:10002 read entries (small
//!   `#p` sets, threshold = 15)
//!
//! NO discovery-kind indexer fan-out — that's the one lane the in-tree
//! tests don't depend on (they pre-seed `kind:10002` explicitly). Tests
//! that need the full algorithm live in `nmp-testing/tests/real_relay_*`
//! and use the router-side `nmp_router::Nip65OutboxResolver` directly.

use std::sync::Arc;

use super::action::{PublishTarget, RelayUrl};
use super::traits::{OutboxResolver, RelaySelectionReason, ResolvedRelay};
use crate::store::{EventStore, PubKey};
use crate::substrate::BlockedRelaySet;

#[derive(Clone)]
pub struct TestKind10002OutboxResolver {
    store: Arc<dyn EventStore>,
    local_write_relays: Option<crate::slots::LocalWriteRelaysSlot>,
    active_account: Option<crate::slots::ActiveAccountSlot>,
}

impl TestKind10002OutboxResolver {
    /// Build a resolver over the given event store.
    #[must_use]
    pub fn new(store: Arc<dyn EventStore>) -> Self {
        Self {
            store,
            local_write_relays: None,
            active_account: None,
        }
    }

    /// Wire the kernel's `local_write_relays_handle` + `active_account_handle`
    /// slots so the resolver can fall back to local rows for the active
    /// account when no kind:10002 is on file yet (mirrors the router-side
    /// `Nip65OutboxResolver::with_local_relays`).
    #[must_use]
    pub fn with_local_relays(
        mut self,
        local_write_relays: crate::slots::LocalWriteRelaysSlot,
        active_account: crate::slots::ActiveAccountSlot,
    ) -> Self {
        self.local_write_relays = Some(local_write_relays);
        self.active_account = Some(active_account);
        self
    }

    fn lookup_relays(&self, author_hex: &str) -> (Vec<RelayUrl>, Vec<RelayUrl>) {
        let Some(author) = hex_to_pubkey(author_hex) else {
            return (Vec::new(), Vec::new());
        };
        let Ok(iter) = self
            .store
            .scan_by_author_kind(&author, &[10002], None, None, 1)
        else {
            return (Vec::new(), Vec::new());
        };
        let Some(Ok(stored)) = iter.into_iter().next() else {
            return (Vec::new(), Vec::new());
        };
        let mut writes = Vec::new();
        let mut reads = Vec::new();
        for tag in &stored.raw.tags {
            if tag.first().map(String::as_str) != Some("r") {
                continue;
            }
            let Some(url) = tag.get(1) else { continue };
            if !(url.starts_with("wss://") || url.starts_with("ws://")) {
                continue;
            }
            match tag.get(2).map(String::as_str) {
                Some("write") => writes.push(url.clone()),
                Some("read") => reads.push(url.clone()),
                None | Some("") => {
                    writes.push(url.clone());
                    reads.push(url.clone());
                }
                Some(_) => {
                    writes.push(url.clone());
                    reads.push(url.clone());
                }
            }
        }
        (writes, reads)
    }

    fn is_active_account(&self, author_pubkey: &str) -> bool {
        let Some(slot) = self.active_account.as_ref() else {
            return false;
        };
        slot.lock()
            .ok()
            .and_then(|guard| guard.clone())
            .is_some_and(|active| active == author_pubkey)
    }
}

impl OutboxResolver for TestKind10002OutboxResolver {
    fn resolve(
        &self,
        author_pubkey: &str,
        p_tags: &[String],
        target: &PublishTarget,
        _kind: u32,
        blocked: &BlockedRelaySet,
    ) -> Vec<ResolvedRelay> {
        if let PublishTarget::Explicit { relays } = target {
            return relays
                .iter()
                .filter(|url| !blocked.contains(url))
                .map(|url| ResolvedRelay {
                    url: url.clone(),
                    reason: RelaySelectionReason::Explicit,
                })
                .collect();
        }
        let mut out: Vec<ResolvedRelay> = Vec::new();
        let (writes, _reads) = self.lookup_relays(author_pubkey);
        for url in writes {
            out.push(ResolvedRelay {
                url,
                reason: RelaySelectionReason::AuthorWriteRelay,
            });
        }

        // Active-account local-write fallback (parity with the router-side
        // `Nip65OutboxResolver`). Applies only when the author IS the
        // active account AND no kind:10002 is on file yet.
        if out.is_empty() && self.is_active_account(author_pubkey) {
            if let Some(slot) = self.local_write_relays.as_ref() {
                if let Ok(guard) = slot.lock() {
                    for url in guard.as_slice().iter().cloned() {
                        out.push(ResolvedRelay {
                            url,
                            reason: RelaySelectionReason::LocalConfigRelay,
                        });
                    }
                }
            }
        }

        // Recipient-inbox fanout for small `#p` sets — mirror the production
        // threshold (15). The canonical constant lives at
        // `nmp_router::RECIPIENT_INBOX_FANOUT_PTAG_THRESHOLD`.
        const RECIPIENT_INBOX_FANOUT_PTAG_THRESHOLD: usize = 15;
        if p_tags.len() < RECIPIENT_INBOX_FANOUT_PTAG_THRESHOLD {
            for p in p_tags {
                let (_writes, reads) = self.lookup_relays(p);
                for url in reads {
                    out.push(ResolvedRelay {
                        url,
                        reason: RelaySelectionReason::RecipientInbox { pubkey: p.clone() },
                    });
                }
            }
        }
        // Blocked-relay post-filter (parity with the production resolver).
        out.retain(|r| !blocked.contains(&r.url));
        out
    }
}

fn hex_to_pubkey(hex: &str) -> Option<PubKey> {
    if hex.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
        let hi = hex_nibble(chunk[0])?;
        let lo = hex_nibble(chunk[1])?;
        out[i] = (hi << 4) | lo;
    }
    Some(out)
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
