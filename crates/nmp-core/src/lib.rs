mod actor;
mod app;
mod ffi;
mod kernel;
pub mod planner;
mod relay;
mod relay_worker;
pub mod store;
pub mod substrate;

pub use app::{AppState, KernelAction, KernelUpdate, KernelViewSpec};
pub use ffi::NmpApp;

// Re-export the FFI entry-points so the ffi-stress harness (and any other
// Rust-side crate) can call them directly via the Rust rlib dependency,
// without an `extern "C"` block. The symbols remain `#[no_mangle]` on the
// ffi:: side and are still reachable from Swift/C unchanged.
#[cfg(any(test, feature = "test-support"))]
pub use ffi::{
    nmp_app_claim_profile, nmp_app_close_author, nmp_app_configure, nmp_app_free,
    nmp_app_inject_events, nmp_app_new, nmp_app_open_author, nmp_app_open_firehose_tag,
    nmp_app_release_profile, nmp_app_set_update_callback, nmp_app_start,
};

/// Test-support facade: gives live-bench binaries access to the actor
/// internals without exposing domain nouns in the stable `nmp-core` API.
///
/// Enable with `features = ["test-support"]` in `Cargo.toml`.  This gate is
/// intentionally `any(test, feature = "test-support")` so `cargo test` always
/// has access without an explicit feature flag.
#[cfg(any(test, feature = "test-support"))]
pub mod testing {
    pub use crate::actor::{run_actor, ActorCommand};

    use std::sync::mpsc;
    use std::thread;

    /// Spawn the kernel actor on a dedicated thread.
    ///
    /// Returns a command sender and an update receiver.  The caller drives the
    /// actor by sending [`ActorCommand`] values and reads JSON-encoded kernel
    /// snapshots from the update channel.  Dropping the sender or sending
    /// [`ActorCommand::Shutdown`] stops the actor thread.
    pub fn spawn_actor() -> (mpsc::Sender<ActorCommand>, mpsc::Receiver<String>) {
        let (command_tx, command_rx) = mpsc::channel();
        let (update_tx, update_rx) = mpsc::channel();
        thread::spawn(move || run_actor(command_rx, update_tx));
        (command_tx, update_rx)
    }
}
