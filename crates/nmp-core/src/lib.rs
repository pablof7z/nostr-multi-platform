mod actor;
mod app;
mod ffi;
mod kernel;
mod relay;
mod relay_worker;
pub mod substrate;

pub use actor::{run_actor, ActorCommand};
pub use app::{AppState, KernelAction, KernelUpdate, KernelViewSpec};
pub use ffi::NmpApp;

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
