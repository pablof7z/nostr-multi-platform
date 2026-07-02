//! Receiving NutZaps — redeem a received kind:9321 event by re-signing its
//! P2PK proofs with the wallet's Cashu key and swapping them at the mint for
//! fresh, spendable proofs.

use tracing::info;

use crate::cashu::client::{self as cashu_client, split_amount, MintClient};
use crate::cashu::types::Proof;
use crate::error::Nip60Error;
use crate::history_event::{build_history_event, HistoryRecord};
use crate::token_event::{build_token_event, TokenRecord};

use super::Nip60WalletHandle;

impl Nip60WalletHandle {
    /// Redeem a received nutzap: swap the P2PK proofs for fresh proofs and
    /// queue a kind:7375 token event plus a kind:7376 history event marking it
    /// redeemed. Queued events are published by the kernel.
    pub fn redeem_nutzap(
        &self,
        nutzap: &crate::nutzap::ReceivedNutZap,
    ) -> Result<u64, Nip60Error> {
        if self.has_redeemed_nutzap(nutzap.event_id) {
            return Err(Nip60Error::AlreadyRedeemed(nutzap.event_id));
        }

        // Sign P2PK proofs with our Cashu private key.
        let config = self.config.lock().unwrap().clone();
        let cashu_sk_bytes = hex::decode(&config.privkey_hex)
            .map_err(|e| Nip60Error::Crypto(format!("cashu privkey: {e}")))?;
        let cashu_sk = nostr::secp256k1::SecretKey::from_slice(&cashu_sk_bytes)
            .map_err(|e| Nip60Error::Crypto(format!("cashu privkey parse: {e}")))?;

        let input_proofs: Vec<Proof> = nutzap
            .proofs
            .iter()
            .map(|np| {
                let proof = Proof {
                    amount: np.amount,
                    id: np.id.clone(),
                    secret: np.secret.clone(),
                    c: np.c.clone(),
                    dleq: np.dleq.clone(),
                    witness: None,
                };
                crate::nutzap::sign_p2pk_proof(&proof, &cashu_sk)
            })
            .collect::<Result<Vec<_>, _>>()?;

        let total: u64 = input_proofs.iter().map(|p| p.amount).sum();

        let client = MintClient::new(&nutzap.mint_url);
        let keyset = client.get_sat_keyset()?;
        let fee = cashu_client::MintClient::compute_fee(input_proofs.len() as u64, keyset.input_fee_ppk);
        let net_total = total.saturating_sub(fee);
        let output_amounts = split_amount(net_total);

        let fresh_proofs = client.swap(input_proofs, output_amounts, None, &keyset)?;

        // Save fresh proofs as a new token event.
        let record = TokenRecord::new(nutzap.mint_url.clone(), fresh_proofs);
        let builder = build_token_event(&record, &self.keys)?;
        let event = builder
            .sign_with_keys(&self.keys)
            .map_err(|e| Nip60Error::Event(format!("sign redeemed token: {e}")))?;
        let new_event_id = event.id;
        self.enqueue(event);

        // Update in-memory state.
        let mut record_with_id = record;
        record_with_id.event_id = Some(new_event_id);
        self.tokens.lock().unwrap().push(record_with_id);

        // Queue history event.
        let mut history = HistoryRecord::new_in(total);
        history.created.push(new_event_id);
        history.redeemed.push(nutzap.event_id);
        if let Ok(h_builder) = build_history_event(&history, &self.keys) {
            if let Ok(h_event) = h_builder.sign_with_keys(&self.keys) {
                self.enqueue(h_event);
            }
        }
        self.mark_redeemed_nutzap(nutzap.event_id);

        info!("redeemed nutzap: {total} sat from {}", nutzap.sender_pubkey.to_hex());
        Ok(total)
    }
}
