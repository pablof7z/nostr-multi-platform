//! `nmp-read-session` — the ONE concept-neutral read-lifecycle engine (#2777).
//!
//! A concept-owned active read (#2508) is a kept-live query with a close
//! handle: open one or more `REQ`s, replay cached/stored matches before live
//! activation, push typed output while the view is mounted, then withdraw the
//! exact demand and tombstone the output on close. Every such read — feed,
//! search, group-feed, and the #2758 reads (`open_replies`, `open_reactions`,
//! …) — shares EXACTLY ONE implementation of those mechanics, which lives here.
//!
//! # Specific outside, generic inside
//!
//! The public doors are concept-owned and concept-named (`nmp_replies::
//! open_replies`, a feed compiler, …); the lifecycle *mechanics* behind them are
//! this single generic engine. This is the inverse of the rejected
//! `open_session(namespace, bytes)` shape (#2508): there is no public generic
//! doorway and no relation buckets — just a private engine each concept drives.
//!
//! # Engine owns vs concept supplies
//!
//! The engine (this crate) owns, with no per-concept variation:
//! - handle allocation + the open/replace/close [`ReadSessionRegistry`];
//! - replay-before-live ordering ([`replay_shapes_for`] derives the read-cache
//!   replay from the concept's demand filter — the seed strategy is a supplied
//!   stage, defaulting to structural replay);
//! - live activation + exact-demand withdrawal (via the host's observed
//!   projection primitive);
//! - reverse teardown ordering ([`ReadSessionRegistry::close`]);
//! - typed-output clear/tombstone on close;
//! - one registry of all open reads → one leak audit
//!   ([`ReadSessionRegistry::live_count`]).
//!
//! The concept owner supplies only DECLARATIVE parts, as a [`ReadSpec`]: the
//! demand(s), the admission-applying event reducer (an
//! [`nmp_core::ObservedProjectionSink`]), and the typed output encoder. A
//! concept owner may *specify* demand/admission/reducer/output; it must not
//! *implement* replay, live activation, registry replacement, exact close,
//! reverse teardown, or tombstone emission — those are [`open_read`] /
//! [`close_read`] / [`ReadSessionRegistry`], never re-authored per concept.
//!
//! # Layering (D0)
//!
//! This crate names NO protocol/product noun: no NIP kinds, no `feed`/`reply`/
//! `search` concept in its mechanics. Concept crates depend on this engine
//! (`nmp-replies → nmp-read-session`); the engine never depends on a concept
//! crate. Runtimes implement the [`ReadHost`] seam once, generically
//! (`NmpApp: ReadHost`), so a concept read needs no per-concept runtime method
//! and browser parity is structural — a new host implements one seam, not one
//! method per concept.

mod engine;
mod host;
pub mod ownership;
mod registry;

pub use engine::{close_read, open_read, replay_shapes_for};
pub use host::{ReadDemand, ReadHandle, ReadHost, ReadOutputEncoder, ReadSpec};
pub use registry::{ReadSessionBuild, ReadSessionId, ReadSessionRegistry, TeardownAction};
