//! Web Locks arbitration for the durable OPFS browser store.
//!
//! Exactly one tab may open the OPFS-SQLite store for a database name. The
//! lock-holder gets durable storage; non-holders start honestly in memory with
//! `second_tab_pool_lock` surfaced through the normal degraded-store diagnostic.

#![cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
pub(super) struct DurableTabLock {
    release: js_sys::Function,
}

#[cfg(target_arch = "wasm32")]
impl DurableTabLock {
    pub(super) fn release(&self) {
        let _ = self.release.call0(&JsValue::NULL);
    }
}

#[cfg(target_arch = "wasm32")]
impl Drop for DurableTabLock {
    fn drop(&mut self) {
        self.release();
    }
}

#[cfg(target_arch = "wasm32")]
pub(super) async fn acquire_durable_tab_lock(name: &str) -> Result<DurableTabLock, String> {
    use std::cell::RefCell;
    use std::rc::Rc;

    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;

    let locks = locks_manager().ok_or_else(|| "Web Locks API unavailable".to_string())?;
    let options = js_sys::Object::new();
    js_sys::Reflect::set(
        &options,
        &JsValue::from_str("mode"),
        &JsValue::from_str("exclusive"),
    )
    .map_err(js_err)?;
    js_sys::Reflect::set(&options, &JsValue::from_str("ifAvailable"), &JsValue::TRUE)
        .map_err(js_err)?;

    let release_cell: Rc<RefCell<Option<js_sys::Function>>> = Rc::new(RefCell::new(None));
    let release_for_callback = Rc::clone(&release_cell);
    let lock_name = name.to_string();

    let acquire_promise = js_sys::Promise::new(&mut |resolve, reject| {
        let resolve_for_callback = resolve.clone();
        let reject_for_request = reject.clone();
        let release_for_callback = Rc::clone(&release_for_callback);
        let callback = Closure::once_into_js(move |lock: JsValue| -> js_sys::Promise {
            if lock.is_null() || lock.is_undefined() {
                let _ = resolve_for_callback.call1(&JsValue::NULL, &JsValue::FALSE);
                return js_sys::Promise::resolve(&JsValue::FALSE);
            }
            let release_for_promise = Rc::clone(&release_for_callback);
            let pending = js_sys::Promise::new(&mut |release, _reject| {
                *release_for_promise.borrow_mut() = Some(release);
            });
            let _ = resolve_for_callback.call1(&JsValue::NULL, &JsValue::TRUE);
            pending
        });
        let request = match js_sys::Reflect::get(&locks, &JsValue::from_str("request"))
            .ok()
            .and_then(|v| v.dyn_into::<js_sys::Function>().ok())
        {
            Some(request) => request,
            None => {
                let _ = reject_for_request.call1(
                    &JsValue::NULL,
                    &JsValue::from_str("navigator.locks.request unavailable"),
                );
                return;
            }
        };
        if let Err(err) = request.call3(&locks, &JsValue::from_str(&lock_name), &options, &callback)
        {
            let _ = reject_for_request.call1(&JsValue::NULL, &err);
        }
    });

    let acquired = JsFuture::from(acquire_promise).await.map_err(js_err)?;
    if !acquired.as_bool().unwrap_or(false) {
        return Err("durable tab lock is already held".to_string());
    }
    let release = release_cell
        .borrow()
        .as_ref()
        .cloned()
        .ok_or_else(|| "durable tab lock release handle missing".to_string())?;
    Ok(DurableTabLock { release })
}

#[cfg(target_arch = "wasm32")]
fn locks_manager() -> Option<JsValue> {
    let global = js_sys::global();
    let navigator = js_sys::Reflect::get(&global, &JsValue::from_str("navigator")).ok()?;
    if navigator.is_null() || navigator.is_undefined() {
        return None;
    }
    let locks = js_sys::Reflect::get(&navigator, &JsValue::from_str("locks")).ok()?;
    if locks.is_null() || locks.is_undefined() {
        None
    } else {
        Some(locks)
    }
}

#[cfg(target_arch = "wasm32")]
fn js_err(value: JsValue) -> String {
    value.as_string().unwrap_or_else(|| format!("{value:?}"))
}
