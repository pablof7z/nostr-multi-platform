use std::cell::Cell;
use std::rc::Rc;

use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_test::*;

use crate::browser_driver::{BrowserKernelHandlers, BrowserRelayDriver};
use crate::browser_timer::BrowserTimer;
use crate::role::RelayRole;

wasm_bindgen_test_configure!(run_in_browser);

struct FakeWebSocketGlobal {
    handle: JsValue,
}

impl FakeWebSocketGlobal {
    fn install() -> Self {
        let handle = js_sys::eval(
            r#"
            (() => {
                const state = { sockets: [] };
                const previous = globalThis.WebSocket;
                class FakeWebSocket {
                    static CONNECTING = 0;
                    static OPEN = 1;
                    static CLOSING = 2;
                    static CLOSED = 3;
                    constructor(url) {
                        this.url = url;
                        this.readyState = FakeWebSocket.CONNECTING;
                        state.sockets.push(this);
                    }
                    set binaryType(value) { this._binaryType = value; }
                    get binaryType() { return this._binaryType; }
                    set onopen(value) { this._onopen = value; }
                    get onopen() { return this._onopen; }
                    set onmessage(value) { this._onmessage = value; }
                    get onmessage() { return this._onmessage; }
                    set onclose(value) { this._onclose = value; }
                    get onclose() { return this._onclose; }
                    set onerror(value) { this._onerror = value; }
                    get onerror() { return this._onerror; }
                    send(_text) {}
                    close() {
                        this.readyState = FakeWebSocket.CLOSED;
                    }
                    __triggerClose(reason = "relay restart") {
                        this.readyState = FakeWebSocket.CLOSED;
                        this._onclose?.(new CloseEvent("close", {
                            code: 1001,
                            reason,
                            wasClean: true,
                        }));
                    }
                }
                globalThis.WebSocket = FakeWebSocket;
                return {
                    state,
                    restore() {
                        globalThis.WebSocket = previous;
                    },
                };
            })()
            "#,
        )
        .expect("install fake WebSocket global");
        Self { handle }
    }

    fn socket(&self, index: u32) -> JsValue {
        let state = js_sys::Reflect::get(&self.handle, &JsValue::from_str("state"))
            .expect("fake websocket state");
        let sockets =
            js_sys::Reflect::get(&state, &JsValue::from_str("sockets")).expect("fake socket list");
        js_sys::Reflect::get(&sockets, &JsValue::from_f64(f64::from(index)))
            .expect("fake socket at index")
    }

    fn socket_count(&self) -> usize {
        let state = js_sys::Reflect::get(&self.handle, &JsValue::from_str("state"))
            .expect("fake websocket state");
        let sockets =
            js_sys::Reflect::get(&state, &JsValue::from_str("sockets")).expect("fake socket list");
        js_sys::Reflect::get(&sockets, &JsValue::from_str("length"))
            .expect("fake socket length")
            .as_f64()
            .expect("numeric fake socket length") as usize
    }
}

impl Drop for FakeWebSocketGlobal {
    fn drop(&mut self) {
        let restore = js_sys::Reflect::get(&self.handle, &JsValue::from_str("restore"))
            .expect("fake websocket restore")
            .dyn_into::<js_sys::Function>()
            .expect("restore function");
        let _ = restore.call0(&self.handle);
    }
}

fn fake_timer() -> BrowserTimer {
    let target = js_sys::eval(
        r#"
        ({
            nextId: 40,
            scheduled: [],
            cleared: [],
            callbacks: new Map(),
            setTimeout(callback, delayMs) {
                const id = ++this.nextId;
                this.scheduled.push({ id, delayMs });
                this.callbacks.set(id, callback);
                return id;
            },
            clearTimeout(id) {
                this.cleared.push(id);
                this.callbacks.delete(id);
            },
            fire(id) {
                const callback = this.callbacks.get(id);
                if (!callback) {
                    return false;
                }
                this.callbacks.delete(id);
                callback();
                return true;
            },
        })
        "#,
    )
    .expect("fake worker timer object");
    BrowserTimer { target }
}

fn timer_array_len(timer: &BrowserTimer, field: &str) -> usize {
    let array =
        js_sys::Reflect::get(&timer.target, &JsValue::from_str(field)).expect("timer array");
    js_sys::Reflect::get(&array, &JsValue::from_str("length"))
        .expect("timer array length")
        .as_f64()
        .expect("numeric timer array length") as usize
}

fn scheduled_timer_id(timer: &BrowserTimer, index: u32) -> i32 {
    let scheduled = js_sys::Reflect::get(&timer.target, &JsValue::from_str("scheduled"))
        .expect("scheduled timer list");
    let entry = js_sys::Reflect::get(&scheduled, &JsValue::from_f64(f64::from(index)))
        .expect("scheduled timer entry");
    js_sys::Reflect::get(&entry, &JsValue::from_str("id"))
        .expect("scheduled timer id")
        .as_f64()
        .expect("numeric scheduled timer id") as i32
}

fn cleared_timer_id(timer: &BrowserTimer, index: u32) -> i32 {
    let cleared =
        js_sys::Reflect::get(&timer.target, &JsValue::from_str("cleared")).expect("cleared");
    js_sys::Reflect::get(&cleared, &JsValue::from_f64(f64::from(index)))
        .expect("cleared timer id")
        .as_f64()
        .expect("numeric cleared timer id") as i32
}

fn fire_timer(timer: &BrowserTimer, id: i32) -> bool {
    let fire = js_sys::Reflect::get(&timer.target, &JsValue::from_str("fire"))
        .expect("timer fire")
        .dyn_into::<js_sys::Function>()
        .expect("timer fire function");
    fire.call1(&timer.target, &JsValue::from_f64(f64::from(id)))
        .expect("fire timer")
        .as_bool()
        .expect("boolean fire result")
}

fn trigger_close(socket: &JsValue) {
    let trigger = js_sys::Reflect::get(socket, &JsValue::from_str("__triggerClose"))
        .expect("trigger close")
        .dyn_into::<js_sys::Function>()
        .expect("trigger close function");
    let _ = trigger.call0(socket).expect("invoke fake close event");
}

fn test_handlers(failed_count: Rc<Cell<u32>>) -> BrowserKernelHandlers {
    BrowserKernelHandlers {
        on_connected: Rc::new(|_, _, _| {}),
        on_text: Rc::new(|_, _, _| {}),
        on_binary: Rc::new(|_, _, _| {}),
        on_close: Rc::new(|_, _, _| {}),
        on_closed: Rc::new(|_, _| {}),
        on_failed: Rc::new(move |_, _, _| {
            failed_count.set(failed_count.get() + 1);
        }),
    }
}

#[wasm_bindgen_test]
fn relay_close_schedules_one_reconnect_on_worker_timer() {
    let websocket = FakeWebSocketGlobal::install();
    let timer = fake_timer();
    let failed_count = Rc::new(Cell::new(0));
    let driver = BrowserRelayDriver::new_with_timer(
        "ws://relay.example.test".to_string(),
        RelayRole::Content,
        test_handlers(Rc::clone(&failed_count)),
        timer.clone(),
    )
    .expect("driver uses fake websocket constructor");

    trigger_close(&websocket.socket(0));

    assert_eq!(timer_array_len(&timer, "scheduled"), 1);
    assert_eq!(timer_array_len(&timer, "cleared"), 0);
    assert_eq!(websocket.socket_count(), 1);

    let id = scheduled_timer_id(&timer, 0);
    assert!(fire_timer(&timer, id));
    assert_eq!(websocket.socket_count(), 2);
    assert_eq!(failed_count.get(), 0);

    drop(driver);
}

#[wasm_bindgen_test]
fn close_cancels_pending_reconnect_timer() {
    let websocket = FakeWebSocketGlobal::install();
    let timer = fake_timer();
    let driver = BrowserRelayDriver::new_with_timer(
        "ws://relay.example.test".to_string(),
        RelayRole::Content,
        test_handlers(Rc::new(Cell::new(0))),
        timer.clone(),
    )
    .expect("driver uses fake websocket constructor");

    trigger_close(&websocket.socket(0));
    let id = scheduled_timer_id(&timer, 0);

    driver.close();

    assert_eq!(timer_array_len(&timer, "cleared"), 1);
    assert_eq!(cleared_timer_id(&timer, 0), id);
    assert!(!fire_timer(&timer, id));
    assert_eq!(websocket.socket_count(), 1);
}
