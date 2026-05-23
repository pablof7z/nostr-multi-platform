//! Browser-side relay driver: one `web_sys::WebSocket` per (URL, role) pair.
//!
//! # V-01 Stage 3 — the wasm32 transport
//!
//! Mirrors the native `nmp_core::relay_worker` thread shape *behaviourally*
//! while using a fundamentally different I/O model. Where the native worker
//! drives `tungstenite` + `mio` from a dedicated OS thread, the browser has
//! neither threads nor a blocking `read_frame`: every inbound frame arrives
//! through a `web_sys::MessageEvent` callback on the main JS event loop. So
//! the protocol-loop split is intentional and unavoidable:
//!
//! | Concern                  | Native (`relay_worker`)              | WASM (`relay_driver`)               |
//! |--------------------------|--------------------------------------|-------------------------------------|
//! | Socket I/O               | `tungstenite` over `TcpStream`+`mio` | `web_sys::WebSocket` callbacks      |
//! | Read loop                | blocking `read()` on poll-readable   | `onmessage` JS closure              |
//! | Reconnect scheduling     | `recv_timeout` on control channel    | `setTimeout` + `Closure` callback   |
//! | Keepalive                | `KeepaliveState` FSM, OS-thread tick | Browser sends Pong automatically    |
//! | Backoff constants        | `relay_protocol::*` ← shared         | `relay_protocol::*` ← shared        |
//! | HTTP 401/403 detection   | `is_permanent_error` ← shared        | `is_permanent_error` ← shared       |
//! | Kernel frame ingest      | `Kernel::handle_message` (private)   | `KernelReducer::handle_relay_frame` |
//!
//! The kernel never knows which transport produced a frame — both paths feed
//! [`nmp_core::RelayFrame`] into the same kernel methods.
//!
//! # Keepalive (browser-native)
//!
//! Browsers transparently respond to server-initiated WebSocket Pings with a
//! Pong before delivering anything to JS — the application can neither send
//! nor observe Ping/Pong frames through `web_sys::WebSocket`. The kernel's
//! `KeepaliveState` FSM (which drives `Message::Ping` writes on native) is
//! therefore not needed here: the user-agent keeps the socket alive, and the
//! kernel's `RelayStatus.connection` flips to `closed` via `oncloseevent`
//! whenever the underlying transport actually drops. This is the spec-correct
//! behaviour for browser WebSockets and matches the V-01 Stage 3 design note.
//!
//! # No polling (D8)
//!
//! Reconnect deadlines are scheduled through `setTimeout` — a one-shot
//! deadline that re-arms only after the next failure. There is no
//! `setInterval` and no sleep+check loop. The driver is purely event-driven.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use nmp_core::{
    relay_protocol::{
        is_permanent_error, jittered_backoff, RELAY_RECONNECT_DELAY_INITIAL,
        RELAY_RECONNECT_DELAY_MAX,
    },
    KernelReducer, OutboundMessage, RelayFrame, RelayRole,
};
use wasm_bindgen::{closure::Closure, JsCast, JsValue};
use web_sys::{BinaryType, CloseEvent, ErrorEvent, MessageEvent, WebSocket};

/// Sink the driver pushes "the kernel produced N outbound frames" reports to.
///
/// The runtime registers this sink at driver-construction time and uses it
/// to: (1) route any outbound the kernel returns into the appropriate sibling
/// driver's [`BrowserRelayDriver::send_text`] (typical: kernel emits an AUTH
/// response or a view REQ in reaction to an inbound frame), and (2) emit a
/// fresh `WorkerEvent::Update` snapshot to the JS host so the UI reflects
/// the new kernel state.
///
/// Substrate-grade (D0): receives only protocol-neutral [`OutboundMessage`]s.
pub type RelaySink = Rc<dyn Fn(Vec<OutboundMessage>)>;

/// Browser-side relay driver — one `web_sys::WebSocket` per (URL, role) pair.
///
/// Held by the runtime behind `Rc<Self>` so the JS closures that drive the
/// reconnect path can call back into the driver without cycles (the closures
/// hold an `Rc<Self>`; dropping the runtime's `Rc` and the live `Closure`
/// references drops the driver and the user-agent reclaims the socket).
pub struct BrowserRelayDriver {
    url: String,
    role: RelayRole,
    state: RefCell<DriverState>,
    /// Shared kernel handle. Every driver in the runtime points at the same
    /// `KernelReducer`; the JS event loop serializes access so concurrent
    /// borrow conflicts are impossible at runtime (a closure that needs the
    /// kernel borrows briefly, then drops).
    kernel: Rc<RefCell<KernelReducer>>,
    /// Outbound + snapshot sink installed by the runtime.
    sink: RelaySink,
}

/// Internal driver state mutated from JS closures.
struct DriverState {
    /// Live socket (None between disconnect and reconnect dials).
    current_socket: Option<WebSocket>,
    /// Current reconnect delay — doubled on each failure, reset to
    /// `RELAY_RECONNECT_DELAY_INITIAL` on a successful connect.
    backoff: Duration,
    /// `true` once the driver has seen at least one `Connected` for this URL.
    /// Every subsequent Connected is a true reconnect — the kernel needs the
    /// T116/G1 replay path.
    has_connected_before: bool,
    /// `true` if the relay has explicitly rejected us (HTTP 401/403) or the
    /// runtime has called `close()`. No more reconnect attempts.
    permanent_failure: bool,
    /// Active JS closures — retained for the socket's lifetime. Replaced on
    /// every reconnect so the old socket's leaks are bounded.
    _closures: SocketClosures,
    /// Currently-armed reconnect `setTimeout` callback. Held so the closure
    /// is not GC'd before the timer fires. Reset on every reconnect attempt.
    _reconnect_timer: Option<Closure<dyn FnMut()>>,
}

/// Holder for the four JS closures wired to a single `WebSocket`. Keeping
/// them in a struct (not a `Vec<JsValue>`) makes the lifetime story explicit:
/// the closures live exactly as long as their owning `DriverState`, and the
/// `Drop` of each `Closure` calls the wasm-bindgen drop hook. Default is "no
/// closures installed" — the placeholder used between reconnects.
///
/// The `dead_code` allowance is load-bearing: the fields are never **read**
/// in Rust, but the JS event loop reads them through the `WebSocket`'s
/// installed handler pointers. Dropping a `Closure` invalidates that handler
/// pointer, which is exactly the leak-bounded reconnect contract.
#[derive(Default)]
#[allow(dead_code)]
struct SocketClosures {
    on_open: Option<Closure<dyn FnMut()>>,
    on_message: Option<Closure<dyn FnMut(MessageEvent)>>,
    on_close: Option<Closure<dyn FnMut(CloseEvent)>>,
    on_error: Option<Closure<dyn FnMut(ErrorEvent)>>,
}

impl BrowserRelayDriver {
    /// Construct a driver and immediately dial the relay. The first connect
    /// happens synchronously (the WebSocket constructor returns); the
    /// `onopen` callback fires asynchronously on the JS event loop.
    ///
    /// Returns `Err(JsValue)` only if the WebSocket constructor itself rejects
    /// the URL (bad scheme, invalid characters). Subsequent connect failures
    /// are surfaced through the kernel via `handle_relay_failed`.
    pub fn new(
        url: String,
        role: RelayRole,
        kernel: Rc<RefCell<KernelReducer>>,
        sink: RelaySink,
    ) -> Result<Rc<Self>, JsValue> {
        let driver = Rc::new(Self {
            url,
            role,
            state: RefCell::new(DriverState {
                current_socket: None,
                backoff: RELAY_RECONNECT_DELAY_INITIAL,
                has_connected_before: false,
                permanent_failure: false,
                _closures: SocketClosures::default(),
                _reconnect_timer: None,
            }),
            kernel,
            sink,
        });
        driver.dial()?;
        Ok(driver)
    }

    /// Send a text frame on the live socket, if any. Silently dropped when
    /// the socket is not currently open — matches the native worker's
    /// per-relay `pending` queue *after* the queue is drained on Connected
    /// (the kernel's own `mark_publish_relay_unavailable` arm handles
    /// re-queueing inside the publish engine).
    pub fn send_text(&self, text: &str) -> Result<(), JsValue> {
        let state = self.state.borrow();
        match &state.current_socket {
            Some(socket) => socket.send_with_str(text),
            None => Ok(()),
        }
    }

    /// Close the socket cleanly and stop any pending reconnect. Idempotent:
    /// subsequent calls are no-ops. Drops the JS closures so the user-agent
    /// reclaims them.
    pub fn close(&self) {
        let mut state = self.state.borrow_mut();
        state.permanent_failure = true;
        if let Some(socket) = state.current_socket.take() {
            let _ = socket.close();
        }
        state._closures = SocketClosures::default();
        state._reconnect_timer = None;
    }

    /// Relay URL this driver dials.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Open a new WebSocket and wire its four event closures. Called once
    /// from `new()` and again from every reconnect path. Each invocation
    /// builds a fresh closure set and overwrites the old socket (which is
    /// closed automatically as the `current_socket` slot is replaced).
    fn dial(self: &Rc<Self>) -> Result<(), JsValue> {
        let socket = WebSocket::new(&self.url)?;
        // ArrayBuffer over Blob — `Blob` would force an async `FileReader`
        // round-trip for every binary frame, which the kernel does not need
        // (it counts bytes and drops). `ArrayBuffer` is synchronous and
        // delivered directly inside the `onmessage` closure.
        socket.set_binary_type(BinaryType::Arraybuffer);

        let on_open = self.build_on_open();
        let on_message = self.build_on_message();
        let on_close = self.build_on_close();
        let on_error = self.build_on_error();

        socket.set_onopen(Some(on_open.as_ref().unchecked_ref()));
        socket.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        socket.set_onclose(Some(on_close.as_ref().unchecked_ref()));
        socket.set_onerror(Some(on_error.as_ref().unchecked_ref()));

        let mut state = self.state.borrow_mut();
        state.current_socket = Some(socket);
        state._closures = SocketClosures {
            on_open: Some(on_open),
            on_message: Some(on_message),
            on_close: Some(on_close),
            on_error: Some(on_error),
        };
        Ok(())
    }

    fn build_on_open(self: &Rc<Self>) -> Closure<dyn FnMut()> {
        let weak = Rc::downgrade(self);
        Closure::wrap(Box::new(move || {
            let Some(driver) = weak.upgrade() else { return };
            // Reset backoff — a successful connect clears the previous
            // failure streak. Snapshot `is_reconnect` BEFORE flipping the
            // `has_connected_before` flag so the kernel routes through the
            // T116/G1 replay path for every connect after the first.
            let is_reconnect = {
                let mut s = driver.state.borrow_mut();
                s.backoff = RELAY_RECONNECT_DELAY_INITIAL;
                let was_connected = s.has_connected_before;
                s.has_connected_before = true;
                was_connected
            };
            let outbound = driver
                .kernel
                .borrow_mut()
                .handle_relay_connected(driver.role, &driver.url, is_reconnect);
            (driver.sink)(outbound);
        }) as Box<dyn FnMut()>)
    }

    fn build_on_message(self: &Rc<Self>) -> Closure<dyn FnMut(MessageEvent)> {
        let weak = Rc::downgrade(self);
        Closure::wrap(Box::new(move |event: MessageEvent| {
            let Some(driver) = weak.upgrade() else { return };
            let Some(frame) = relay_frame_from_message(&event) else {
                return;
            };
            let outbound = driver
                .kernel
                .borrow_mut()
                .handle_relay_frame(driver.role, &driver.url, frame);
            (driver.sink)(outbound);
        }) as Box<dyn FnMut(MessageEvent)>)
    }

    fn build_on_close(self: &Rc<Self>) -> Closure<dyn FnMut(CloseEvent)> {
        let weak = Rc::downgrade(self);
        Closure::wrap(Box::new(move |event: CloseEvent| {
            let Some(driver) = weak.upgrade() else { return };
            let reason = event.reason();
            // Hand a Close frame to the kernel so `relay.last_close_reason`
            // surfaces in the next snapshot. Outbound from a Close is always
            // empty so we ignore the return.
            let _ = driver.kernel.borrow_mut().handle_relay_frame(
                driver.role,
                &driver.url,
                RelayFrame::Close(if reason.is_empty() {
                    None
                } else {
                    Some(reason.clone())
                }),
            );
            driver
                .kernel
                .borrow_mut()
                .handle_relay_closed(driver.role, &driver.url);

            // Clear current socket — the user-agent already dropped it.
            driver.state.borrow_mut().current_socket = None;

            // Decide reconnect vs. give up. Two skip conditions only —
            // matches the native `run_connected_relay` exit branches
            // (`Shutdown` and `PermanentFailure`):
            //   1. `permanent_failure` — `BrowserRelayDriver::close()` was
            //      called by the host, mirroring native's `RelayCommand::Shutdown`.
            //   2. `is_permanent_error(reason)` — HTTP 401/403 (or the literal
            //      "Forbidden" token) in the close reason, mirroring native's
            //      `RelayWorkerResult::PermanentFailure` from the same classifier.
            //
            // `event.was_clean()` is NOT a skip condition: a relay that closes
            // gracefully with code 1001 ("going away" — planned restart) or
            // exchanges close frames before tearing down for a config reload
            // still fires `wasClean=true`, and the native worker reconnects on
            // both. Skipping on `was_clean` would silently strand the driver
            // every time the relay does a clean restart.
            let permanent =
                driver.state.borrow().permanent_failure || is_permanent_error(&reason);
            if !permanent {
                driver.schedule_reconnect();
            }
        }) as Box<dyn FnMut(CloseEvent)>)
    }

    fn build_on_error(self: &Rc<Self>) -> Closure<dyn FnMut(ErrorEvent)> {
        let weak = Rc::downgrade(self);
        Closure::wrap(Box::new(move |event: ErrorEvent| {
            let Some(driver) = weak.upgrade() else { return };
            let message = event.message();
            // ErrorEvent on a WebSocket is followed by a CloseEvent — the
            // close handler owns the reconnect decision. We only report the
            // error string into the kernel so the snapshot surfaces it.
            driver.kernel.borrow_mut().handle_relay_failed(
                driver.role,
                &driver.url,
                if message.is_empty() {
                    "websocket error".to_string()
                } else {
                    message
                },
            );
        }) as Box<dyn FnMut(ErrorEvent)>)
    }

    /// Schedule a reconnect via `setTimeout`. Each call doubles the backoff
    /// up to [`RELAY_RECONNECT_DELAY_MAX`] and applies the `jittered_backoff`
    /// spread so simultaneous failures across many relays don't all reconnect
    /// on the same tick. The closure is retained in `state._reconnect_timer`
    /// so the JS GC doesn't drop it before the deadline.
    fn schedule_reconnect(self: &Rc<Self>) {
        let delay = {
            let mut s = self.state.borrow_mut();
            let delay = jittered_backoff(s.backoff, &self.url);
            s.backoff = (s.backoff * 2).min(RELAY_RECONNECT_DELAY_MAX);
            delay
        };
        let window = match web_sys::window() {
            Some(w) => w,
            None => return, // no window (e.g. worker without `self`) — give up
        };
        let weak = Rc::downgrade(self);
        let cb = Closure::wrap(Box::new(move || {
            let Some(driver) = weak.upgrade() else { return };
            // Re-dial — if it fails synchronously (bad URL is rare here since
            // it worked the first time, but the user-agent may reject under
            // memory pressure), drop the timer and report the failure.
            if let Err(error) = driver.dial() {
                let error_str = format!("reconnect dial failed: {error:?}");
                driver.kernel.borrow_mut().handle_relay_failed(
                    driver.role,
                    &driver.url,
                    error_str,
                );
            }
        }) as Box<dyn FnMut()>);
        let result = window.set_timeout_with_callback_and_timeout_and_arguments_0(
            cb.as_ref().unchecked_ref(),
            i32::try_from(delay.as_millis()).unwrap_or(i32::MAX),
        );
        // Park the closure in state so JS does not GC it before firing.
        // setTimeout returning Err means the user-agent refused the schedule;
        // we drop the closure (no leak) and surface no reconnect attempt.
        if result.is_ok() {
            self.state.borrow_mut()._reconnect_timer = Some(cb);
        }
    }
}

/// Convert a `web_sys::MessageEvent` into a [`RelayFrame`]. Returns `None`
/// for payload types the kernel never observes (e.g. JS objects that are
/// neither strings nor `ArrayBuffer`s). NIP-01 traffic is exclusively text
/// over the wire; the binary path exists only to count bytes for the
/// `RelayStatus.frames_rx` diagnostic.
fn relay_frame_from_message(event: &MessageEvent) -> Option<RelayFrame> {
    let data: JsValue = event.data();
    if let Some(text) = data.as_string() {
        return Some(RelayFrame::Text(text));
    }
    if let Ok(buffer) = data.dyn_into::<js_sys::ArrayBuffer>() {
        let bytes = js_sys::Uint8Array::new(&buffer).to_vec();
        return Some(RelayFrame::Binary(bytes));
    }
    None
}
