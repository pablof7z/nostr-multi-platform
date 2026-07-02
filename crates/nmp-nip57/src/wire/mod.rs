//! Typed FlatBuffers wire codecs for `nmp-nip57` action payloads (ADR-0071).
//!
//! `zap_payload` holds the WRITE-direction [`nmp_core::substrate::ActionPayload`]
//! impl for [`crate::ZapInput`] (S9 #1747).

pub mod zap_payload;
