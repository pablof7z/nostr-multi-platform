use crate::actor::{run_actor, ActorCommand};
use crate::kernel::{is_hex_id, is_hex_pubkey};
use crate::relay::{DEFAULT_EMIT_HZ, DEFAULT_VISIBLE_LIMIT};
use std::ffi::{c_char, c_uint, c_void, CStr, CString};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::thread::JoinHandle;

type UpdateCallback = extern "C" fn(*mut c_void, *const c_char);

#[derive(Clone, Copy)]
struct UpdateCallbackRegistration {
    context: usize,
    callback: UpdateCallback,
}

pub struct NmpApp {
    tx: Sender<ActorCommand>,
    update_callback: Arc<Mutex<Option<UpdateCallbackRegistration>>>,
    actor: Mutex<Option<JoinHandle<()>>>,
    update_listener: Mutex<Option<JoinHandle<()>>>,
}

impl Drop for NmpApp {
    fn drop(&mut self) {
        if let Ok(mut callback) = self.update_callback.lock() {
            *callback = None;
        }
        let _ = self.tx.send(ActorCommand::Shutdown);
        if let Ok(mut actor) = self.actor.lock() {
            if let Some(handle) = actor.take() {
                let _ = handle.join();
            }
        }
        if let Ok(mut listener) = self.update_listener.lock() {
            if let Some(handle) = listener.take() {
                let _ = handle.join();
            }
        }
    }
}

#[no_mangle]
pub extern "C" fn nmp_app_new() -> *mut NmpApp {
    let (command_tx, command_rx) = mpsc::channel();
    let (update_tx, update_rx) = mpsc::channel();
    let update_callback: Arc<Mutex<Option<UpdateCallbackRegistration>>> =
        Arc::new(Mutex::new(None));
    let listener_callback = Arc::clone(&update_callback);
    let actor = thread::spawn(move || run_actor(command_rx, update_tx));
    let update_listener = thread::spawn(move || {
        while let Ok(update) = update_rx.recv() {
            let Ok(payload) = CString::new(update) else {
                continue;
            };
            let callback = listener_callback.lock().ok().and_then(|guard| *guard);
            if let Some(registration) = callback {
                (registration.callback)(registration.context as *mut c_void, payload.as_ptr());
            }
        }
    });

    Box::into_raw(Box::new(NmpApp {
        tx: command_tx,
        update_callback,
        actor: Mutex::new(Some(actor)),
        update_listener: Mutex::new(Some(update_listener)),
    }))
}

#[no_mangle]
pub extern "C" fn nmp_app_free(app: *mut NmpApp) {
    if !app.is_null() {
        unsafe {
            drop(Box::from_raw(app));
        }
    }
}

#[no_mangle]
pub extern "C" fn nmp_app_set_update_callback(
    app: *mut NmpApp,
    context: *mut c_void,
    callback: Option<UpdateCallback>,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Ok(mut slot) = app.update_callback.lock() else {
        return;
    };
    *slot = callback.map(|callback| UpdateCallbackRegistration {
        context: context as usize,
        callback,
    });
}

#[no_mangle]
pub extern "C" fn nmp_app_start(
    app: *mut NmpApp,
    _events_per_second: c_uint,
    visible_limit: c_uint,
    emit_hz: c_uint,
) {
    let Some(app) = app_ref(app) else {
        return;
    };

    let _ = app.tx.send(ActorCommand::Start {
        visible_limit: clamp_visible(visible_limit),
        emit_hz: clamp_emit_hz(emit_hz),
    });
}

#[no_mangle]
pub extern "C" fn nmp_app_configure(
    app: *mut NmpApp,
    _events_per_second: c_uint,
    visible_limit: c_uint,
    emit_hz: c_uint,
) {
    let Some(app) = app_ref(app) else {
        return;
    };

    let _ = app.tx.send(ActorCommand::Configure {
        visible_limit: clamp_visible(visible_limit),
        emit_hz: clamp_emit_hz(emit_hz),
    });
}

#[no_mangle]
pub extern "C" fn nmp_app_stop(app: *mut NmpApp) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let _ = app.tx.send(ActorCommand::Stop);
}

#[no_mangle]
pub extern "C" fn nmp_app_reset(app: *mut NmpApp) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let _ = app.tx.send(ActorCommand::Reset);
}

#[no_mangle]
pub extern "C" fn nmp_app_open_author(app: *mut NmpApp, pubkey: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(pubkey) = c_string_argument(pubkey) else {
        return;
    };
    if !is_hex_pubkey(&pubkey) {
        return;
    }

    let _ = app.tx.send(ActorCommand::OpenAuthor { pubkey });
}

#[no_mangle]
pub extern "C" fn nmp_app_open_thread(app: *mut NmpApp, event_id: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(event_id) = c_string_argument(event_id) else {
        return;
    };
    if !is_hex_id(&event_id) {
        return;
    }

    let _ = app.tx.send(ActorCommand::OpenThread { event_id });
}

#[no_mangle]
pub extern "C" fn nmp_app_open_firehose_tag(app: *mut NmpApp, tag: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(tag) = c_string_argument(tag) else {
        return;
    };

    let _ = app.tx.send(ActorCommand::OpenFirehoseTag { tag });
}

#[no_mangle]
pub extern "C" fn nmp_app_claim_profile(
    app: *mut NmpApp,
    pubkey: *const c_char,
    consumer_id: *const c_char,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(pubkey) = c_string_argument(pubkey) else {
        return;
    };
    let Some(consumer_id) = c_string_argument(consumer_id) else {
        return;
    };
    if !is_hex_pubkey(&pubkey) {
        return;
    }

    let _ = app.tx.send(ActorCommand::ClaimProfile {
        pubkey,
        consumer_id,
    });
}

#[no_mangle]
pub extern "C" fn nmp_app_release_profile(
    app: *mut NmpApp,
    pubkey: *const c_char,
    consumer_id: *const c_char,
) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(pubkey) = c_string_argument(pubkey) else {
        return;
    };
    let Some(consumer_id) = c_string_argument(consumer_id) else {
        return;
    };
    if !is_hex_pubkey(&pubkey) {
        return;
    }

    let _ = app.tx.send(ActorCommand::ReleaseProfile {
        pubkey,
        consumer_id,
    });
}

#[no_mangle]
pub extern "C" fn nmp_app_close_author(app: *mut NmpApp, pubkey: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(pubkey) = c_string_argument(pubkey) else {
        return;
    };
    if !is_hex_pubkey(&pubkey) {
        return;
    }

    let _ = app.tx.send(ActorCommand::CloseAuthor { pubkey });
}

#[no_mangle]
pub extern "C" fn nmp_app_close_thread(app: *mut NmpApp, event_id: *const c_char) {
    let Some(app) = app_ref(app) else {
        return;
    };
    let Some(event_id) = c_string_argument(event_id) else {
        return;
    };
    if !is_hex_id(&event_id) {
        return;
    }

    let _ = app.tx.send(ActorCommand::CloseThread { event_id });
}

fn app_ref<'a>(app: *mut NmpApp) -> Option<&'a NmpApp> {
    if app.is_null() {
        None
    } else {
        Some(unsafe { &*app })
    }
}

fn c_string_argument(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        return None;
    }

    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .ok()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn clamp_visible(visible_limit: c_uint) -> usize {
    if visible_limit == 0 {
        DEFAULT_VISIBLE_LIMIT
    } else {
        visible_limit.clamp(1, 500) as usize
    }
}

fn clamp_emit_hz(emit_hz: c_uint) -> u32 {
    if emit_hz == 0 {
        DEFAULT_EMIT_HZ
    } else {
        emit_hz.clamp(1, 12)
    }
}
