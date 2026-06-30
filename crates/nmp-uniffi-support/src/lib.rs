//! Shared Rust-side mechanics for UniFFI facade contracts.
//!
//! This crate intentionally does **not** call `uniffi::setup_scaffolding!()`.
//! A native app links exactly one UniFFI cdylib, and that owning facade crate
//! calls `setup_scaffolding!()` once. Exported UniFFI records and callback
//! traits must therefore live in the owning facade crate's namespace; this
//! crate only shares the panic containment, quiescence, dispatch, and clamp
//! mechanics behind those facade-local types.

use std::sync::Arc;

use nmp_core::__ffi_internal::{dispatch_capability, NativeCapabilityHandler};
use nmp_core::substrate::ActionResult;
use nmp_native_runtime::{
    dispatch_action_bytes_typed, NmpApp, UpdateListener, DEFAULT_EMIT_HZ, DEFAULT_VISIBLE_LIMIT,
};

/// Typed outcome of a `dispatch_action` call.
///
/// Exactly one of `correlation_id` (accepted) or `error` (rejected/failed)
/// will be `Some`. `code` is `Some` only for coded rejections.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DispatchOutcome {
    pub correlation_id: Option<String>,
    pub error: Option<String>,
    pub code: Option<String>,
}

impl From<nmp_native_runtime::DispatchOutcome> for DispatchOutcome {
    fn from(out: nmp_native_runtime::DispatchOutcome) -> Self {
        DispatchOutcome {
            correlation_id: out.correlation_id,
            error: out.error,
            code: out.code,
        }
    }
}

impl DispatchOutcome {
    #[must_use]
    pub fn error(message: impl Into<String>) -> Self {
        DispatchOutcome {
            correlation_id: None,
            error: Some(message.into()),
            code: None,
        }
    }
}

/// Dispatch an NMPD FlatBuffers action envelope through the native runtime.
#[must_use]
pub fn dispatch_action(app: &NmpApp, envelope: &[u8]) -> DispatchOutcome {
    dispatch_action_bytes_typed(app, envelope).into()
}

/// Owned-`Vec` convenience for UniFFI facade methods.
#[must_use]
pub fn dispatch_action_vec(app: &NmpApp, envelope: Vec<u8>) -> DispatchOutcome {
    dispatch_action(app, &envelope)
}

/// Register or clear the update sink on a native runtime app.
pub fn set_update_sink<S, F>(app: &NmpApp, sink: Option<Box<S>>, on_update: F)
where
    S: Send + Sync + 'static + ?Sized,
    F: Fn(&S, Vec<u8>) + Send + Sync + 'static,
{
    app.set_update_listener(sink.map(|sink| update_listener_from_sink(sink, on_update)));
}

/// Convert a UniFFI update sink into the native runtime listener shape.
#[must_use]
pub fn update_listener_from_sink<S, F>(sink: Box<S>, on_update: F) -> UpdateListener
where
    S: Send + Sync + 'static + ?Sized,
    F: Fn(&S, Vec<u8>) + Send + Sync + 'static,
{
    let sink: Arc<S> = Arc::from(sink);
    let on_update = Arc::new(on_update);
    Arc::new(move |bytes: &[u8]| {
        let frame = bytes.to_vec();
        let sink = Arc::clone(&sink);
        let on_update = Arc::clone(&on_update);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            on_update(sink.as_ref(), frame);
        }));
    }) as UpdateListener
}

/// Register or clear the native capability callback.
pub fn set_capability_callback<S, F>(app: &NmpApp, sink: Option<Box<S>>, on_request: F)
where
    S: Send + Sync + 'static + ?Sized,
    F: Fn(&S, String) -> String + Send + Sync + 'static,
{
    app.capability_callback_slot()
        .set_native_handler(sink.map(|sink| capability_handler_from_sink(sink, on_request)));
}

/// Convert a UniFFI capability sink into the native runtime handler shape.
#[must_use]
pub fn capability_handler_from_sink<S, F>(sink: Box<S>, on_request: F) -> NativeCapabilityHandler
where
    S: Send + Sync + 'static + ?Sized,
    F: Fn(&S, String) -> String + Send + Sync + 'static,
{
    let sink: Arc<S> = Arc::from(sink);
    let on_request = Arc::new(on_request);
    Arc::new(move |request_json: String| -> String {
        let req_for_call = request_json.clone();
        let sink = Arc::clone(&sink);
        let on_request = Arc::clone(&on_request);
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            on_request(sink.as_ref(), req_for_call)
        }));
        result.unwrap_or_else(|_| {
            nmp_core::__ffi_internal::capability_error_envelope(&request_json, "sink-panicked")
        })
    }) as NativeCapabilityHandler
}

/// Route a capability request JSON through the registered handler.
#[must_use]
pub fn dispatch_capability_json(app: &NmpApp, request_json: &str) -> String {
    dispatch_capability(&app.capability_callback_slot(), request_json)
}

/// Register an action-result observer and serialize pushed results as JSON.
pub fn register_action_result_observer<S, F>(app: &NmpApp, observer: Box<S>, on_result: F)
where
    S: Send + Sync + 'static + ?Sized,
    F: Fn(&S, String) + Send + Sync + 'static,
{
    let observer: Arc<S> = Arc::from(observer);
    let on_result = Arc::new(on_result);
    app.register_action_result_observer(move |result: ActionResult| {
        let Ok(json) = serde_json::to_string(&result) else {
            return;
        };
        let observer = Arc::clone(&observer);
        let on_result = Arc::clone(&on_result);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            on_result(observer.as_ref(), json);
        }));
    });
}

/// Clear the action-result observer and wait for in-flight callbacks to drain.
pub fn clear_action_result_observer(app: &NmpApp) {
    app.clear_action_result_observer();
}

/// Register or clear the lifecycle observer.
pub fn set_lifecycle_callback<S, F>(app: &NmpApp, sink: Option<Box<S>>, on_phase: F)
where
    S: Send + Sync + 'static + ?Sized,
    F: Fn(&S, u32) + Send + Sync + 'static,
{
    app.set_native_lifecycle_observer(
        sink.map(|sink| lifecycle_observer_from_sink(sink, on_phase)),
    );
}

/// Convert a UniFFI lifecycle sink into the native runtime observer shape.
#[must_use]
pub fn lifecycle_observer_from_sink<S, F>(
    sink: Box<S>,
    on_phase: F,
) -> nmp_core::__ffi_internal::NativeLifecycleObserver
where
    S: Send + Sync + 'static + ?Sized,
    F: Fn(&S, u32) + Send + Sync + 'static,
{
    let sink: Arc<S> = Arc::from(sink);
    let on_phase = Arc::new(on_phase);
    Arc::new(move |phase: u32| {
        let sink = Arc::clone(&sink);
        let on_phase = Arc::clone(&on_phase);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
            on_phase(sink.as_ref(), phase);
        }));
    })
}

/// Clamp `visible_limit` identically for all UniFFI facades.
#[must_use]
pub fn clamp_visible(visible_limit: u32) -> usize {
    if visible_limit == 0 {
        DEFAULT_VISIBLE_LIMIT
    } else {
        visible_limit.clamp(1, 500) as usize
    }
}

/// Clamp `emit_hz` identically for all UniFFI facades.
#[must_use]
pub fn clamp_emit_hz(emit_hz: u32) -> u32 {
    if emit_hz == 0 {
        DEFAULT_EMIT_HZ
    } else {
        emit_hz.clamp(1, 12)
    }
}

/// Start a runtime through the shared UniFFI clamp contract.
pub fn start_runtime(app: &NmpApp, visible_limit: u32, emit_hz: u32) {
    app.start_runtime(clamp_visible(visible_limit), clamp_emit_hz(emit_hz));
}

/// Reconfigure a runtime through the shared UniFFI clamp contract.
pub fn configure_runtime(app: &NmpApp, visible_limit: u32, emit_hz: u32) {
    app.configure_runtime(clamp_visible(visible_limit), clamp_emit_hz(emit_hz));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamp_contract_matches_runtime_defaults() {
        assert_eq!(clamp_visible(0), DEFAULT_VISIBLE_LIMIT);
        assert_eq!(clamp_visible(999), 500);
        assert_eq!(clamp_visible(10), 10);

        assert_eq!(clamp_emit_hz(0), DEFAULT_EMIT_HZ);
        assert_eq!(clamp_emit_hz(99), 12);
        assert_eq!(clamp_emit_hz(4), 4);
    }

    #[test]
    fn dispatch_empty_envelope_returns_error_outcome() {
        let app = nmp_native_runtime::new_app();
        let out = dispatch_action(&app, &[]);
        assert!(out.correlation_id.is_none());
        assert!(out.error.is_some());
    }
}

/// Compiled ownership descriptor for crate-ownership reports.
pub mod ownership;
