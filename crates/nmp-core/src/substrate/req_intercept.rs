//! `ReqFrameInterceptor` — substrate-generic outbound REQ rewrite seam.
//!
//! Some protocol crates can replace a raw NIP-01 `REQ` with a more efficient
//! relay-side sync protocol, then emit follow-up `REQ`s only for missing event
//! ids. The kernel owns the planner and the transport choke point, but the
//! protocol crate owns the wire dialect. This trait keeps that boundary
//! explicit: the actor offers each planner-produced REQ to one installed
//! interceptor, and falls back to the original REQ when the interceptor declines.

use std::sync::{Arc, Mutex};

use crate::kernel::Kernel;
use crate::planner::{InterestId, InterestLifecycle};
use crate::relay::{OutboundMessage, RelayRole};

/// Immutable description of a planner-produced outbound `REQ`.
#[derive(Clone, Debug)]
pub struct ReqFrameContext {
    /// Transport lane label used for diagnostics and auth gating.
    pub role: RelayRole,
    /// Canonical relay URL the frame is addressed to.
    pub relay_url: String,
    /// Wire subscription id assigned by the planner.
    pub sub_id: String,
    /// NIP-01 filter JSON object, without the surrounding `["REQ", ...]`.
    pub filter_json: String,
    /// Logical interest that originated this sub-shape.
    pub interest_id: InterestId,
    /// Lifecycle attached to the logical interest.
    pub lifecycle: InterestLifecycle,
}

/// Outbound REQ rewrite hook owned by a protocol crate.
pub trait ReqFrameInterceptor: Send + Sync + 'static {
    /// Return replacement outbound frames, or `None` to let the actor send
    /// the original raw `REQ`.
    fn intercept_req(
        &self,
        kernel: &mut Kernel,
        ctx: &ReqFrameContext,
    ) -> Option<Vec<OutboundMessage>>;
}

/// Shared slot holding the active outbound REQ interceptor.
pub type ReqFrameInterceptorSlot = Arc<Mutex<Option<Arc<dyn ReqFrameInterceptor>>>>;

/// Construct a fresh empty [`ReqFrameInterceptorSlot`].
#[must_use]
pub fn new_req_frame_interceptor_slot() -> ReqFrameInterceptorSlot {
    Arc::new(Mutex::new(None))
}
