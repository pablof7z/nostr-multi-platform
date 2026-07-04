//! Typed FlatBuffers wire codecs for NIP-AD projections.

mod ad_collection_fb;

pub use ad_collection_fb::{
    decode_ad_collection_snapshot, encode_ad_collection_snapshot, FILE_IDENTIFIER, SCHEMA_ID,
    SCHEMA_VERSION,
};
