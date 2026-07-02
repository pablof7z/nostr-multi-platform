//! Typed FlatBuffers wire codec for the `nmp.threading.graph` read model.
//!
//! Split into its own module (mirroring `nmp-nip25::wire`) so the generated
//! bindings and the hand-written codec stay separately reviewable.

mod threading_graph_fb;

pub use threading_graph_fb::{
    decode_threading_snapshot, encode_threading_snapshot, THREADING_GRAPH_FILE_IDENTIFIER,
    THREADING_GRAPH_SCHEMA_ID, THREADING_GRAPH_SCHEMA_VERSION,
};
