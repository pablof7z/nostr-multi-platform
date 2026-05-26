#[allow(
    clippy::all,
    dead_code,
    deprecated,
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    unused_imports
)]
#[path = "generated/nmp_update_generated.rs"]
pub mod nmp_update_generated;

pub use nmp_update_generated::nmp::transport as wire;
