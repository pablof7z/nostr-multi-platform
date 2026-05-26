//! `EventClaimSink` — renderer→host bridge for upstream event fetches.
//!
//! ADR-0034 / M16. The trait lives in `nmp-content` so renderers (e.g.
//! `NostrContentView` in the TUI registry) can take
//! `Option<&dyn EventClaimSink>` without `nmp-content` ever gaining an
//! `nmp-ffi` dependency. Each platform host (TUI, iOS, Compose) supplies an
//! impl that bridges `claim` / `release` to `nmp_app_claim_event` /
//! `nmp_app_release_event` on the FFI surface.

/// Host-side bridge that lets a renderer initiate an upstream fetch for
/// an embedded event (ADR-0034). The trait lives in nmp-content so
/// nmp-content never gains an nmp-ffi dependency; each platform host
/// supplies the impl that bridges to its FFI surface.
///
/// # Examples
///
/// ```
/// use nmp_content::EventClaimSink;
///
/// struct MyHost;
/// impl EventClaimSink for MyHost {
///     fn claim(&self, _uri: &str, _consumer_id: &str) { /* call FFI */ }
///     fn release(&self, _uri: &str, _consumer_id: &str) { /* call FFI */ }
/// }
/// let _: Box<dyn EventClaimSink> = Box::new(MyHost);
/// ```
pub trait EventClaimSink: Send + Sync {
    /// Initiate (or refcount-increment) an upstream fetch for `uri` on
    /// behalf of `consumer_id`. Implementations are expected to be
    /// idempotent and infallible — failure must be swallowed silently so
    /// renderers can call this on every render pass without guarding.
    fn claim(&self, uri: &str, consumer_id: &str);

    /// Release a previously-claimed `(uri, consumer_id)` pair. A
    /// double-release or unknown pair is a no-op.
    fn release(&self, uri: &str, consumer_id: &str);
}

/// No-op sink — fixture/test surfaces use this so renderers can run
/// without an active kernel.
pub struct NoopEventClaimSink;

impl EventClaimSink for NoopEventClaimSink {
    fn claim(&self, _uri: &str, _consumer_id: &str) {}
    fn release(&self, _uri: &str, _consumer_id: &str) {}
}
