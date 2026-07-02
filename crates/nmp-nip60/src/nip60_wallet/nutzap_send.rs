//! Sending NutZaps — proof selection, P2PK-locking, mint swap, and
//! publishing the kind:9321 nutzap event plus the kind:10019 NutZap info
//! event that advertises this wallet's accepted mints.

use tracing::info;

use nostr::{EventId, PublicKey};

use crate::cashu::client::{self as cashu_client, split_amount, MintClient};
use crate::cashu::types::Proof;
use crate::error::Nip60Error;
use crate::nutzap::{build_nutzap_info_event, p2pk_secret, NutZapInfo, NutZapProof};
use crate::token_event::{build_token_event, TokenRecord};

use super::Nip60WalletHandle;

impl Nip60WalletHandle {
    /// Send a NutZap to a recipient.
    ///
    /// The caller supplies the recipient's kind:10019 NutZap info (which the
    /// kernel fetched from its store). This method swaps proofs for
    /// P2PK-locked proofs at the mint, then queues the kind:9321 nutzap event
    /// in the outbox for the kernel to publish.
    ///
    /// Returns the queued nutzap event id.
    pub fn send_nutzap(
        &self,
        amount_sats: u64,
        recipient_pubkey: &PublicKey,
        nutzap_info: &NutZapInfo,
        comment: Option<&str>,
        zapped_event_id: Option<&EventId>,
    ) -> Result<EventId, Nip60Error> {
        let recipient_mint = nutzap_info
            .mints
            .first()
            .ok_or_else(|| {
                Nip60Error::MintDiscovery("recipient has no accepted mints in NutZap info".into())
            })?
            .clone();
        let recipient_cashu_pubkey = nutzap_info
            .cashu_pubkey
            .clone()
            .unwrap_or_else(|| recipient_pubkey.to_hex());

        // Get P2PK locked proofs.
        let proofs = self.create_p2pk_proofs(amount_sats, &recipient_cashu_pubkey, &recipient_mint)?;
        let nutzap_proofs: Vec<NutZapProof> = proofs.into_iter().map(Into::into).collect();

        // Build and queue nutzap event.
        let nutzap_builder = crate::nutzap::build_nutzap_event(
            nutzap_proofs,
            &recipient_mint,
            recipient_pubkey,
            comment,
            zapped_event_id,
        )?;
        let nutzap_event = nutzap_builder
            .sign_with_keys(&self.keys)
            .map_err(|e| Nip60Error::Event(format!("sign nutzap: {e}")))?;
        let nutzap_event_id = nutzap_event.id;
        self.enqueue(nutzap_event);

        info!(
            "queued nutzap: {} sat to {}",
            amount_sats,
            recipient_pubkey.to_hex()
        );
        Ok(nutzap_event_id)
    }

    /// Build the user's kind:10019 NutZap info event and queue it in the
    /// outbox for the kernel to publish. Returns the queued event id.
    pub fn publish_nutzap_info(&self) -> Result<EventId, Nip60Error> {
        let config = self.config.lock().unwrap().clone();
        let cashu_pubkey = config.pubkey_hex()?;
        let info = NutZapInfo {
            relays: self.relays.clone(),
            mints: config.mints.clone(),
            cashu_pubkey: Some(cashu_pubkey),
        };
        let builder = build_nutzap_info_event(&info, &self.keys)?;
        let event = builder
            .sign_with_keys(&self.keys)
            .map_err(|e| Nip60Error::Event(format!("sign nutzap info: {e}")))?;
        let event_id = event.id;
        self.enqueue(event);
        Ok(event_id)
    }

    /// Select proofs to cover `amount_sats`, swap them at the mint for
    /// P2PK-locked output proofs, and queue the token bookkeeping (spent
    /// deletions + change) that results. Shared by [`Self::send_nutzap`] and
    /// the [`crate::backend::WalletBackend`] adapter.
    pub(super) fn create_p2pk_proofs(
        &self,
        amount_sats: u64,
        recipient_cashu_pubkey: &str,
        mint_url: &str,
    ) -> Result<Vec<Proof>, Nip60Error> {
        // Select proofs from our wallet to cover the amount.
        let (selected, _change_amount) = self.select_proofs(amount_sats, mint_url)?;
        let selected_total: u64 = selected.iter().map(|p| p.amount).sum();

        let client = MintClient::new(mint_url);
        let keyset = client.get_sat_keyset()?;

        // Build P2PK locked secrets for the output denominations.
        let output_amounts = split_amount(amount_sats);
        let p2pk_secrets: Vec<String> = output_amounts
            .iter()
            .map(|_| p2pk_secret(recipient_cashu_pubkey))
            .collect();

        // Compute mint fee and deduct from change.
        let fee = cashu_client::MintClient::compute_fee(selected.len() as u64, keyset.input_fee_ppk);
        let gross_change = selected_total - amount_sats;
        if gross_change < fee {
            return Err(Nip60Error::InsufficientBalance {
                have: selected_total,
                need: amount_sats + fee,
            });
        }
        let change_amount = gross_change - fee;

        let mut all_output_amounts = output_amounts.clone();
        let mut all_secrets: Vec<String> = p2pk_secrets.clone();
        if change_amount > 0 {
            let change_denoms = split_amount(change_amount);
            all_output_amounts.extend_from_slice(&change_denoms);
            for _ in &change_denoms {
                all_secrets.push(hex::encode(crate::cashu::crypto::random_secret()));
            }
        }

        let new_proofs = client.swap(
            selected.clone(),
            all_output_amounts.clone(),
            Some(all_secrets),
            &keyset,
        )?;

        // Split into nutzap proofs and change proofs.
        let nutzap_count = output_amounts.len();
        let nutzap_proofs: Vec<Proof> = new_proofs[..nutzap_count].to_vec();
        let change_proofs: Vec<Proof> = new_proofs[nutzap_count..].to_vec();

        // Delete spent token events and save change.
        self.update_tokens_after_spend(&selected, change_proofs, mint_url)?;

        Ok(nutzap_proofs)
    }

    /// Select proofs summing to at least `amount_sats` from `mint_url`.
    fn select_proofs(
        &self,
        amount_sats: u64,
        mint_url: &str,
    ) -> Result<(Vec<Proof>, u64), Nip60Error> {
        let tokens = self.tokens.lock().unwrap();
        let mut selected = Vec::new();
        let mut total = 0u64;

        for record in tokens.iter().filter(|r| r.mint_url == mint_url) {
            for proof in &record.proofs {
                if total >= amount_sats {
                    break;
                }
                selected.push(proof.clone());
                total += proof.amount;
            }
            if total >= amount_sats {
                break;
            }
        }
        drop(tokens);

        if total < amount_sats {
            return Err(Nip60Error::InsufficientBalance {
                have: total,
                need: amount_sats,
            });
        }
        Ok((selected, total - amount_sats))
    }

    /// Update token events after spending: queue a NIP-09 deletion for each
    /// fully-spent token event, queue a change token event, and refresh
    /// in-memory state. Queued events are published by the kernel.
    fn update_tokens_after_spend(
        &self,
        spent_proofs: &[Proof],
        change_proofs: Vec<Proof>,
        mint_url: &str,
    ) -> Result<(), Nip60Error> {
        let spent_secrets: std::collections::HashSet<&str> =
            spent_proofs.iter().map(|p| p.secret.as_str()).collect();

        let mut tokens = self.tokens.lock().unwrap();
        let mut destroyed_event_ids: Vec<EventId> = Vec::new();
        let mut new_tokens: Vec<TokenRecord> = Vec::new();
        let mut queued: Vec<nostr::Event> = Vec::new();

        for record in tokens.drain(..) {
            let remaining: Vec<Proof> = record
                .proofs
                .into_iter()
                .filter(|p| !spent_secrets.contains(p.secret.as_str()))
                .collect();
            if remaining.is_empty() {
                if let Some(id) = record.event_id {
                    destroyed_event_ids.push(id);
                    // Queue deletion event (NIP-09 — kind:5).
                    if let Ok(del_builder) = self.build_delete_event(id) {
                        if let Ok(del_event) = del_builder.sign_with_keys(&self.keys) {
                            queued.push(del_event);
                        }
                    }
                }
            } else {
                new_tokens.push(TokenRecord {
                    mint_url: record.mint_url,
                    proofs: remaining,
                    del: record.del,
                    event_id: record.event_id,
                });
            }
        }

        // Queue change token event.
        if !change_proofs.is_empty() {
            let change_record = TokenRecord::new(mint_url.to_string(), change_proofs.clone());
            let builder = build_token_event(&change_record, &self.keys)?;
            let event = builder
                .sign_with_keys(&self.keys)
                .map_err(|e| Nip60Error::Event(format!("sign change token: {e}")))?;
            let id = event.id;
            queued.push(event);
            new_tokens.push(TokenRecord {
                mint_url: mint_url.to_string(),
                proofs: change_proofs,
                del: destroyed_event_ids.iter().map(|id| id.to_hex()).collect(),
                event_id: Some(id),
            });
        }

        *tokens = new_tokens;
        drop(tokens);

        for event in queued {
            self.enqueue(event);
        }
        Ok(())
    }

    fn build_delete_event(&self, event_id: EventId) -> Result<nostr::EventBuilder, Nip60Error> {
        use nostr::{EventBuilder, Kind, Tag};
        Ok(EventBuilder::new(Kind::EventDeletion, "spent").tag(Tag::event(event_id)))
    }
}
