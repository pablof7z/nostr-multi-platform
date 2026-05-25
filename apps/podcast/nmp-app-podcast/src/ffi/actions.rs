//! Podcast-specific action-registration helpers invoked from
//! [`super::register::nmp_app_podcast_register`].
//!
//! At M0.A there are no podcast-specific action modules yet — the canonical
//! NMP composition (`nmp_app_template::register_defaults`) wires the generic
//! NIP-02 / NIP-17 / NIP-57 / NIP-65 modules, which is sufficient for the
//! skeleton. Podcast-domain action modules land here as later milestones ship
//! (podcast-core, podcast-feeds, etc.).
//!
//! # Future
//!
//! When `podcast-core::register`, `podcast-feeds::register`, etc. are
//! available they will be called from a `register_podcast_actions(app)` helper
//! in this file, analogous to Chirp's `register_nip29_actions`. The call site
//! in `register.rs` is already structured to receive that call.
