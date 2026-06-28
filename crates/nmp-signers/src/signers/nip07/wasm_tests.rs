use js_sys::{Function, Object, Reflect};
use nmp_signer_iface::SignerError;
use nostr::PublicKey;
use wasm_bindgen::JsValue;
use wasm_bindgen_test::*;

use super::{nip44_available, nip44_via_extension, Nip44Verb};
use crate::signers::traits::Signer;
use crate::signers::Nip07Signer;

wasm_bindgen_test_configure!(run_in_browser);

const SAMPLE_PK: &str = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";

#[wasm_bindgen_test]
fn nip44_capability_requires_encrypt_and_decrypt() {
    install_nip44(
        Some("return Promise.resolve('ciphertext')"),
        Some("return Promise.resolve('plaintext')"),
    );
    assert!(nip44_available());

    let signer = Nip07Signer::from_cached_pubkey(sample_pubkey());
    assert!(Signer::nip44(&signer).is_some());

    install_nip44(Some("return Promise.resolve('ciphertext')"), None);
    assert!(!nip44_available());
    assert!(Signer::nip44(&signer).is_none());
}

#[wasm_bindgen_test(async)]
async fn nip44_encrypt_decrypt_success() {
    install_nip44(
        Some("return Promise.resolve('enc:' + pubkey + ':' + payload)"),
        Some("return Promise.resolve('dec:' + pubkey + ':' + payload)"),
    );
    let encrypted = nip44_via_extension(Nip44Verb::Encrypt, SAMPLE_PK, "hello")
        .await
        .expect("encrypt");
    assert_eq!(encrypted, format!("enc:{SAMPLE_PK}:hello"));

    let decrypted = nip44_via_extension(Nip44Verb::Decrypt, SAMPLE_PK, "ciphertext")
        .await
        .expect("decrypt");
    assert_eq!(decrypted, format!("dec:{SAMPLE_PK}:ciphertext"));
}

#[wasm_bindgen_test(async)]
async fn nip44_missing_namespace_is_unsupported() {
    clear_nostr();
    let err = nip44_via_extension(Nip44Verb::Encrypt, SAMPLE_PK, "hello")
        .await
        .expect_err("missing window.nostr must fail");
    assert_unsupported_contains(err, "window.nostr");

    install_nostr_without_nip44();
    let err = nip44_via_extension(Nip44Verb::Decrypt, SAMPLE_PK, "ciphertext")
        .await
        .expect_err("missing window.nostr.nip44 must fail");
    assert_unsupported_contains(err, "window.nostr.nip44");
}

#[wasm_bindgen_test(async)]
async fn nip44_rejected_and_thrown_errors_are_structured() {
    install_nip44(
        Some("return Promise.reject(new Error('denied'))"),
        Some("return Promise.resolve('plaintext')"),
    );
    let err = nip44_via_extension(Nip44Verb::Encrypt, SAMPLE_PK, "hello")
        .await
        .expect_err("rejection must fail");
    assert!(matches!(err, SignerError::Rejected(_)));

    install_nip44(
        Some("throw new Error('boom')"),
        Some("return Promise.resolve('plaintext')"),
    );
    let err = nip44_via_extension(Nip44Verb::Encrypt, SAMPLE_PK, "hello")
        .await
        .expect_err("throw must fail");
    assert_backend_contains(err, "invocation threw");
}

#[wasm_bindgen_test(async)]
async fn nip44_malformed_return_is_backend_error() {
    install_nip44(
        Some("return Promise.resolve({ ciphertext: 'not-a-string' })"),
        Some("return Promise.resolve('plaintext')"),
    );
    let err = nip44_via_extension(Nip44Verb::Encrypt, SAMPLE_PK, "hello")
        .await
        .expect_err("object return must fail");
    assert_backend_contains(err, "non-string");
}

fn sample_pubkey() -> PublicKey {
    PublicKey::from_hex(SAMPLE_PK).expect("sample pubkey")
}

fn clear_nostr() {
    let window = web_sys::window().expect("browser window");
    Reflect::delete_property(&window, &JsValue::from_str("nostr")).expect("delete nostr");
}

fn install_nostr_without_nip44() {
    let nostr = Object::new();
    let window = web_sys::window().expect("browser window");
    Reflect::set(&window, &JsValue::from_str("nostr"), &nostr).expect("set nostr");
}

fn install_nip44(encrypt_body: Option<&str>, decrypt_body: Option<&str>) {
    let nostr = Object::new();
    let nip44 = Object::new();
    if let Some(body) = encrypt_body {
        let encrypt = Function::new_with_args("pubkey, payload", body);
        Reflect::set(&nip44, &JsValue::from_str("encrypt"), &encrypt).expect("set encrypt");
    }
    if let Some(body) = decrypt_body {
        let decrypt = Function::new_with_args("pubkey, payload", body);
        Reflect::set(&nip44, &JsValue::from_str("decrypt"), &decrypt).expect("set decrypt");
    }
    Reflect::set(&nostr, &JsValue::from_str("nip44"), &nip44).expect("set nip44");
    let window = web_sys::window().expect("browser window");
    Reflect::set(&window, &JsValue::from_str("nostr"), &nostr).expect("set nostr");
}

fn assert_unsupported_contains(err: SignerError, needle: &str) {
    match err {
        SignerError::Unsupported(msg) => assert!(msg.contains(needle), "{msg}"),
        other => panic!("expected Unsupported, got {other:?}"),
    }
}

fn assert_backend_contains(err: SignerError, needle: &str) {
    match err {
        SignerError::Backend(msg) => assert!(msg.contains(needle), "{msg}"),
        other => panic!("expected Backend, got {other:?}"),
    }
}
