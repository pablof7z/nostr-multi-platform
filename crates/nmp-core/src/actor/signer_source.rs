//! Signer source payloads for actor identity commands.

/// Where a signer added via `ActorCommand::AddSigner` comes from.
///
/// Replaces the per-source `SignInNsec` / `SignInBunker` / `AddRemoteSigner`
/// command split: the source kind is now a payload of one unified command.
///
/// D0: the `RemoteHandle` arm carries a `Box<dyn RemoteSignerHandle>` whose
/// concrete type lives in `nmp-signers` — `nmp-core` only sees the trait object
/// (defined in `crate::remote_signer`); it never imports the broker or signer
/// crate.
#[allow(dead_code)] // live cross-crate constructors in nmp-ffi — per-crate lint false positive
pub enum SignerSource {
    /// Local secret key — a `nsec1…` bech32 or 64-hex string. Resolves
    /// synchronously: the actor parses it and (when `make_active`) activates it
    /// immediately. Carried as `Zeroizing<String>` so the plaintext secret is
    /// wiped from memory the instant the command is dropped.
    LocalNsec(zeroize::Zeroizing<String>),
    /// Local secret key for an app-managed signer slot. The actor registers it
    /// in the signer roster, persists it as app-managed local material, and
    /// keeps it hidden from user account projections and active-account
    /// switching.
    AppManagedLocalNsec(zeroize::Zeroizing<String>),
    /// NIP-46 `bunker://` URI. Triggers an asynchronous broker handshake: the
    /// actor seeds the `bunker_handshake` projection, stashes `make_active`, and
    /// delegates the connect/get_public_key dance to the registered broker.
    BunkerUri(String),
    /// A fully-handshaken remote signer handle. The broker adapter constructs
    /// this after a NIP-46 handshake completes and sends it back to the actor.
    RemoteHandle(Box<dyn nmp_signer_iface::RemoteSignerHandle>),
}

impl std::fmt::Debug for SignerSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SignerSource::LocalNsec(_) => f.write_str("LocalNsec(<redacted>)"),
            SignerSource::AppManagedLocalNsec(_) => f.write_str("AppManagedLocalNsec(<redacted>)"),
            SignerSource::BunkerUri(uri) => f.debug_tuple("BunkerUri").field(uri).finish(),
            SignerSource::RemoteHandle(handle) => f
                .debug_tuple("RemoteHandle")
                .field(&handle.pubkey_hex())
                .finish(),
        }
    }
}
