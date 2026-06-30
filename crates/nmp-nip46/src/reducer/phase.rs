use nostr::PublicKey;

pub(crate) enum Phase {
    /// bunker://: waiting for the bunker's `connect` response.
    BunkerWaitConnectAck {
        connect_id: String,
        remote_pubkey: PublicKey,
    },
    /// Both flows: waiting for the `get_public_key` response.
    WaitGpk {
        gpk_id: String,
        remote_pubkey: PublicKey,
    },
    /// nostrconnect://: waiting for the signer's initial `connect` frame.
    NostrConnectWaitConnect { expected_secret: String },
    /// Terminal; further inputs are no-ops.
    Done,
}
