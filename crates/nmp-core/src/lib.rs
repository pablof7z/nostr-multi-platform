mod actor;
mod app;
mod ffi;
mod kernel;
mod relay;
mod relay_worker;
pub mod substrate;

pub use app::{AppState, KernelAction, KernelUpdate, KernelViewSpec};
pub use ffi::NmpApp;
