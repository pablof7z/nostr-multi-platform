//! End-to-end proof: blossom upload terminal surfaces `url` + `sha256` in the
//! kernel-owned `action_results` projection, keyed by the dispatch-returned
//! `correlation_id` (issue #1648 / ADR-0043 Decision 4).
//!
//! This is the canonical completion carrier — NOT `register_action_result_observer`
//! (which fires on accept/enqueue only).

use std::ffi::{c_void, CStr, CString};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use nmp_blossom::{parse_upload_completion, completion_url_sha256};
use nmp_core::decode_snapshot_typed_projections;
use nmp_core::typed_projections::{
    decode_action_results, ACTION_RESULTS_SCHEMA_ID,
};
use nmp_ffi::{
    nmp_app_dispatch_action, nmp_app_free, nmp_app_new, nmp_app_set_update_callback,
    nmp_app_signin_nsec, nmp_app_start, nmp_free_string, NmpApp,
};

const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";

static SERIAL: Mutex<()> = Mutex::new(());
static FRAME_TX: OnceLock<Mutex<Option<Sender<Vec<u8>>>>> = OnceLock::new();

extern "C" fn capture_frame(_ctx: *mut c_void, ptr: *const u8, len: usize) {
    if ptr.is_null() || len == 0 {
        return;
    }
    let bytes = unsafe { std::slice::from_raw_parts(ptr, len) }.to_vec();
    if let Some(slot) = FRAME_TX.get() {
        if let Ok(guard) = slot.lock() {
            if let Some(tx) = guard.as_ref() {
                let _ = tx.send(bytes);
            }
        }
    }
}

fn install_frame_capture() -> Receiver<Vec<u8>> {
    let (tx, rx) = channel();
    let slot = FRAME_TX.get_or_init(|| Mutex::new(None));
    *slot.lock().unwrap() = Some(tx);
    rx
}

fn uninstall_frame_capture() {
    if let Some(slot) = FRAME_TX.get() {
        *slot.lock().unwrap() = None;
    }
}

fn wait_for_upload_terminal(
    rx: &Receiver<Vec<u8>>,
    correlation_id: &str,
) -> Result<serde_json::Value, ()> {
    loop {
        let bytes = rx.recv_timeout(Duration::from_secs(10)).map_err(|_| ())?;
        let Ok(typed) = decode_snapshot_typed_projections(&bytes) else {
            continue;
        };
        let Some(sidecar) = typed.iter().find(|t| t.key == ACTION_RESULTS_SCHEMA_ID) else {
            continue;
        };
        let Ok(model) = decode_action_results(&sidecar.payload) else {
            continue;
        };
        let Some(row) = model
            .results
            .into_iter()
            .find(|r| r.correlation_id == correlation_id)
        else {
            continue;
        };
        if row.status != "published" {
            return Err(());
        }
        let Some(result_str) = row.result else {
            return Err(());
        };
        let result: serde_json::Value =
            serde_json::from_str(&result_str).map_err(|_| ())?;
        return Ok(result);
    }
}

/// Minimal mock Blossom server — returns a fixed descriptor on PUT /upload.
fn spawn_mock_blossom(descriptor_json: &'static str) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://{addr}");
    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = vec![0u8; 8192];
            let _ = stream.read(&mut buf);
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nContent-Type: application/json\r\n\r\n{descriptor_json}",
                descriptor_json.len()
            );
            let _ = stream.write_all(resp.as_bytes());
            let _ = stream.flush();
        }
    });
    url
}

fn dispatch(app: *mut NmpApp, body: &str) -> serde_json::Value {
    let ns = CString::new("nmp.blossom.upload").unwrap();
    let b = CString::new(body).unwrap();
    let ptr = nmp_app_dispatch_action(app, ns.as_ptr(), b.as_ptr());
    assert!(!ptr.is_null());
    let out = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_owned();
    nmp_free_string(ptr);
    serde_json::from_str(&out).unwrap()
}

#[test]
fn upload_terminal_surfaces_url_and_sha256_in_action_results() {
    let _g = SERIAL.lock().unwrap_or_else(|p| p.into_inner());
    let rx = install_frame_capture();

    let expected_sha = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
    let expected_url = format!("https://blossom.test/{expected_sha}.bin");
    let descriptor = format!(
        r#"{{"url":"{expected_url}","sha256":"{expected_sha}","size":5,"type":"application/octet-stream","uploaded":1733356800}}"#
    );
    let descriptor_static: &'static str = Box::leak(descriptor.into_boxed_str());
    let server = spawn_mock_blossom(descriptor_static);

    let path = std::env::temp_dir().join(format!("nmp-blossom-completion-{}.bin", std::process::id()));
    std::fs::write(&path, b"hello").unwrap();

    let app = nmp_app_new();
    nmp_app_set_update_callback(app, std::ptr::null_mut(), Some(capture_frame));
    // SAFETY: valid until nmp_app_free.
    nmp_blossom::register_actions(unsafe { &mut *app });
    nmp_app_start(app, 256, 8);

    let nsec = CString::new(TEST_NSEC).unwrap();
    nmp_app_signin_nsec(app, nsec.as_ptr(), 1);

    let body = serde_json::json!({
        "file_path": path.to_str().unwrap(),
        "content_type": "application/octet-stream",
        "servers": [server],
    })
    .to_string();

    let parsed = dispatch(app, &body);
    let cid = parsed
        .get("correlation_id")
        .and_then(|v| v.as_str())
        .expect("dispatch mints correlation_id")
        .to_string();

    let result = wait_for_upload_terminal(&rx, &cid)
        .unwrap_or_else(|_| panic!("action_results terminal never arrived for cid={cid}"));
    let completion = parse_upload_completion(&result).expect("parse terminal result");
    let (url, sha256) = completion_url_sha256(&completion);
    assert_eq!(sha256, expected_sha, "terminal carries streamed sha256");
    assert_eq!(url, expected_url, "terminal carries server url");

    uninstall_frame_capture();
    let _ = std::fs::remove_file(&path);
    nmp_app_free(app);
}