//! Typed FlatBuffers wire codec for threading graph snapshots.

// The generated FlatBuffers bindings are intrinsically `unsafe` (every accessor
// reads from a raw `Table`). This single generated module opts back into
// `unsafe`; hand-written code in this module tree does not use `unsafe`.
#[allow(
    clippy::all,
    dead_code,
    deprecated,
    missing_docs,
    non_camel_case_types,
    non_snake_case,
    unsafe_code,
    unused_imports
)]
#[path = "generated/threading_graph_generated.rs"]
pub mod generated;

mod decode;
mod encode;

pub use decode::decode_threading_snapshot;
pub use encode::encode_threading_snapshot;

pub(crate) use generated::nmp::threading as fb;

/// FlatBuffers file identifier embedded in every threading graph buffer.
pub const THREADING_GRAPH_FILE_IDENTIFIER: &[u8; 4] = b"NTHR";
/// Wire schema version. Bump on breaking changes to `threading_graph.fbs`.
pub const THREADING_GRAPH_SCHEMA_VERSION: u32 = 1;
