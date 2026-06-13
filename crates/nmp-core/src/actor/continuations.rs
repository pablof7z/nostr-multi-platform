//! Boxed actor-port continuations (always-compiled).
//!
//! [`SignContinuation`] and [`CipherContinuation`] are the resolution callbacks
//! the backend-transparent signer-port commands carry
//! (`ActorCommand::SignEventForAccount` and the ADR-0050 §D1
//! `Nip44{Encrypt,Decrypt}ForAccount` cipher verbs). They live here — outside
//! the `native`-gated `pending_sign` module — because the `ActorCommand` enum
//! that names them is always-compiled (only the actor *runtime* that consumes it
//! is `native`-gated), so they must compile on `wasm32` / no-`native` builds
//! too. Each is a newtype over a boxed `FnOnce` so the enum's derived `Debug`
//! compiles (the inner closure is neither `Debug` nor inspectable; the `Debug`
//! impls print a fixed placeholder).
//!
//! Both run on the actor thread and may only enqueue further work — never block
//! (D8). Neither ever receives key material — only a `SignedEvent` / ciphertext
//! / plaintext (D13). On failure each is called with `Err(reason)` so the
//! worker's failure path runs and the host terminal clears (D6).

/// Boxed continuation invoked with the resolved sign outcome (the generic
/// `ActorCommand::SignEventForAccount` port).
pub struct SignContinuation(
    pub Box<dyn FnOnce(Result<crate::substrate::SignedEvent, String>) + Send>,
);

impl SignContinuation {
    /// Construct from any `FnOnce` matching the sign-outcome shape.
    #[must_use]
    pub fn new(
        f: impl FnOnce(Result<crate::substrate::SignedEvent, String>) + Send + 'static,
    ) -> Self {
        Self(Box::new(f))
    }

    /// Invoke the continuation with the sign outcome, consuming it.
    pub fn call(self, outcome: Result<crate::substrate::SignedEvent, String>) {
        (self.0)(outcome);
    }
}

impl std::fmt::Debug for SignContinuation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SignContinuation(<sign-account continuation>)")
    }
}

/// Boxed continuation invoked with the resolved NIP-44 cipher outcome — the
/// `String`-payload twin of [`SignContinuation`] (ADR-0050 §D1). Resolved with
/// the ciphertext (encrypt) / plaintext (decrypt), or `Err(reason)` on failure.
pub struct CipherContinuation(pub Box<dyn FnOnce(Result<String, String>) + Send>);

impl CipherContinuation {
    /// Construct from any `FnOnce` matching the cipher-outcome shape.
    #[must_use]
    pub fn new(f: impl FnOnce(Result<String, String>) + Send + 'static) -> Self {
        Self(Box::new(f))
    }

    /// Invoke the continuation with the cipher outcome, consuming it.
    pub fn call(self, outcome: Result<String, String>) {
        (self.0)(outcome);
    }
}

impl std::fmt::Debug for CipherContinuation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CipherContinuation(<nip44 cipher continuation>)")
    }
}
