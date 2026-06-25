//! Worker-compatible one-shot browser timers for the wasm relay driver.

use std::time::Duration;

use wasm_bindgen::{closure::Closure, JsCast, JsValue};

pub(crate) struct ReconnectTimer {
    pub(crate) id: i32,
    pub(crate) _callback: Closure<dyn FnMut()>,
}

#[derive(Clone)]
pub(crate) struct BrowserTimer {
    pub(crate) target: JsValue,
}

impl BrowserTimer {
    pub(crate) fn global() -> Self {
        Self {
            target: js_sys::global().into(),
        }
    }

    pub(crate) fn set_timeout(
        &self,
        cb: &Closure<dyn FnMut()>,
        delay: Duration,
    ) -> Result<i32, JsValue> {
        let set_timeout = js_sys::Reflect::get(&self.target, &JsValue::from_str("setTimeout"))?
            .dyn_into::<js_sys::Function>()?;
        let timeout_ms = i32::try_from(delay.as_millis()).unwrap_or(i32::MAX);
        let id = set_timeout.call2(
            &self.target,
            cb.as_ref().unchecked_ref(),
            &JsValue::from_f64(f64::from(timeout_ms)),
        )?;
        timeout_id_from_js(id)
    }

    pub(crate) fn clear_timeout(&self, id: i32) {
        let Ok(clear_timeout) =
            js_sys::Reflect::get(&self.target, &JsValue::from_str("clearTimeout"))
        else {
            return;
        };
        let Ok(clear_timeout) = clear_timeout.dyn_into::<js_sys::Function>() else {
            return;
        };
        let _ = clear_timeout.call1(&self.target, &JsValue::from_f64(f64::from(id)));
    }
}

fn timeout_id_from_js(id: JsValue) -> Result<i32, JsValue> {
    let Some(value) = id.as_f64() else {
        return Err(JsValue::from_str(
            "setTimeout did not return a numeric timeout id",
        ));
    };
    if !value.is_finite() || value.fract() != 0.0 {
        return Err(JsValue::from_str(
            "setTimeout returned an invalid timeout id",
        ));
    }
    if value < f64::from(i32::MIN) || value > f64::from(i32::MAX) {
        return Err(JsValue::from_str(
            "setTimeout returned an out-of-range timeout id",
        ));
    }
    Ok(value as i32)
}
