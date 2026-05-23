//! `SignerOp<T>` — the non-`BoxedFuture` thunk type used by every signer
//! operation that may take time to complete.
//!
//! `SignerOp` is a synchronous-by-default value: for in-memory signers every
//! `sign` / `encrypt` / `decrypt` call is CPU-bound and returns
//! [`SignerOp::Ready`] immediately.  For NIP-46 the result depends on a
//! remote response, so `sign` returns [`SignerOp::Pending`] holding a
//! [`std::sync::mpsc::Receiver`] that the caller can poll.
//!
//! This lets the kernel actor (which is `std::sync::mpsc`-based, not Tokio)
//! integrate signer operations without pulling in an async runtime — the
//! actor's main `select`-style loop can `try_recv()` on pending signer ops
//! the same way it polls relay channels today.

use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError};
use std::time::Duration;

use crate::error::SignerError;

/// Async-or-sync signer operation result.
///
/// `Ready(Ok(t))` and `Ready(Err(e))` cover the synchronous path.  `Pending(rx)`
/// carries a receiver that produces exactly one `Result<T, SignerError>` value
/// when the operation completes (or is dropped on cancel).
pub enum SignerOp<T: Send + 'static> {
    /// Operation completed synchronously.
    Ready(Result<T, SignerError>),
    /// Operation is pending — poll `rx` for the result.
    Pending(Receiver<Result<T, SignerError>>),
}

impl<T: Send + 'static> SignerOp<T> {
    /// Construct a ready-now success.
    pub fn ok(value: T) -> Self {
        Self::Ready(Ok(value))
    }

    /// Construct a ready-now error.
    #[must_use] 
    pub fn err(error: SignerError) -> Self {
        Self::Ready(Err(error))
    }

    /// Block the current thread for up to `timeout` waiting for the result.
    ///
    /// Returns:
    /// - `Ok(result)` if the operation completed in time
    /// - `Err(SignerError::Timeout)` on timeout
    /// - `Err(SignerError::Backend)` if the sender was dropped without
    ///   producing a value
    pub fn wait(self, timeout: Duration) -> Result<T, SignerError> {
        match self {
            Self::Ready(r) => r,
            Self::Pending(rx) => match rx.recv_timeout(timeout) {
                Ok(r) => r,
                Err(RecvTimeoutError::Timeout) => Err(SignerError::Timeout(format!(
                    "signer op did not complete within {timeout:?}"
                ))),
                Err(RecvTimeoutError::Disconnected) => Err(SignerError::Backend(
                    "signer op channel disconnected before completion".to_string(),
                )),
            },
        }
    }

    /// Non-blocking poll.  Returns `None` if still pending, `Some(result)` if
    /// completed.  Disconnect surfaces as `Some(Err(SignerError::Backend))`.
    pub fn poll(&mut self) -> Option<Result<T, SignerError>> {
        match self {
            Self::Ready(_) => {
                let taken = std::mem::replace(
                    self,
                    Self::Ready(Err(SignerError::Backend(
                        "already polled to completion".to_string(),
                    ))),
                );
                match taken {
                    Self::Ready(r) => Some(r),
                    // `self` was `Ready` immediately above, and `replace`
                    // returns that same value — but surface a `Backend` error
                    // rather than panicking at this public boundary.
                    Self::Pending(_) => Some(Err(SignerError::Backend(
                        "signer op poll observed inconsistent state".to_string(),
                    ))),
                }
            }
            Self::Pending(rx) => match rx.try_recv() {
                Ok(r) => Some(r),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => Some(Err(SignerError::Backend(
                    "signer op channel disconnected before completion".to_string(),
                ))),
            },
        }
    }
}

impl<T: Send + 'static> std::fmt::Debug for SignerOp<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ready(Ok(_)) => f.write_str("SignerOp::Ready(Ok(..))"),
            Self::Ready(Err(e)) => write!(f, "SignerOp::Ready(Err({e}))"),
            Self::Pending(_) => f.write_str("SignerOp::Pending(..)"),
        }
    }
}
