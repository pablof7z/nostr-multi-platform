//! `EventRefResolver` — renderer→host bridge for upstream event fetches.
//!
//! ADR-0034 / M16. The trait lives in `nmp-content` so renderers (e.g.
//! `NostrContentView` in the TUI registry) can take
//! `Option<&dyn EventRefResolver>` without `nmp-content` ever gaining an
//! `nmp-ffi` dependency. Each platform host (TUI, iOS, Compose) supplies an
//! impl that decodes the embed URI at the app boundary and bridges `resolve_event_ref` /
//! `release_event_ref` to the unified `resolve_ref` / `release_ref` surface.

/// Host-side bridge that lets a renderer initiate an upstream fetch for
/// an embedded event (ADR-0034). The trait lives in nmp-content so
/// nmp-content never gains an nmp-ffi dependency; each platform host
/// supplies the impl that bridges to its FFI surface. URI decoding is
/// app-owned; the kernel boundary receives the raw event key plus optional
/// relay hints.
///
/// # Examples
///
/// ```
/// use nmp_content::EventRefResolver;
///
/// struct MyHost;
/// impl EventRefResolver for MyHost {
///     fn resolve_event_ref(&self, _uri: &str, _consumer_id: &str) { /* call FFI */ }
///     fn release_event_ref(&self, _uri: &str, _consumer_id: &str) { /* call FFI */ }
/// }
/// let _: Box<dyn EventRefResolver> = Box::new(MyHost);
/// ```
pub trait EventRefResolver: Send + Sync {
    /// Initiate (or refcount-increment) an upstream fetch for `uri` on
    /// behalf of `consumer_id`. Implementations are expected to be
    /// idempotent and infallible — failure must be swallowed silently so
    /// renderers can call this on every render pass without guarding.
    fn resolve_event_ref(&self, uri: &str, consumer_id: &str);

    /// Release a previously-resolved `(uri, consumer_id)` pair. A
    /// double-release or unknown pair is a no-op.
    fn release_event_ref(&self, uri: &str, consumer_id: &str);
}

/// No-op sink — fixture/test surfaces use this so renderers can run
/// without an active kernel.
pub struct NoopEventRefResolver;

impl EventRefResolver for NoopEventRefResolver {
    fn resolve_event_ref(&self, _uri: &str, _consumer_id: &str) {}
    fn release_event_ref(&self, _uri: &str, _consumer_id: &str) {}
}
