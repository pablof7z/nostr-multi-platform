//! Typed FlatBuffers wire codecs for NIP-23 projections.

pub mod longform_fb;

pub use longform_fb::{
    decode_longform_articles, encode_longform_articles, LongformArticles,
    FILE_IDENTIFIER as LONGFORM_FILE_IDENTIFIER, SCHEMA_ID as LONGFORM_SCHEMA_ID,
    SCHEMA_VERSION as LONGFORM_SCHEMA_VERSION,
};
