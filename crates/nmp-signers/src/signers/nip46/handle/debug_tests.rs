use crate::Nip46SignerHandle;

const SAMPLE_PK: &str = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";

#[test]
fn handle_debug_redacts_bunker_secret_and_local_secret() {
    let uri = format!("bunker://{SAMPLE_PK}?relay=wss://relay.example.com&secret=secret-token");
    let handle = Nip46SignerHandle::from_bunker_uri(&uri).expect("parse");

    let s = format!("{handle:?}");
    assert!(s.contains("Nip46SignerHandle"));
    assert!(s.contains(SAMPLE_PK));
    assert!(s.contains("wss://relay.example.com"));
    assert!(!s.contains("secret-token"));
}
