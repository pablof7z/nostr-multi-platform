//! Shared Arc-sink + panic-containment mechanics for UniFFI callback shapes
//! (update, capability, action-result, lifecycle). Split out of `lib.rs` for
//! file-size discipline.

use std::sync::Arc;

use nmp_core::__ffi_internal::{dispatch_capability, NativeCapabilityHandler};
use nmp_core::substrate::ActionResult;
use nmp_native_runtime::{NmpApp, UpdateListener};

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
