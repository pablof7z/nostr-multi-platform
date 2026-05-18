//! Generate a fresh `nsec1...` bech32 secret to stdout.
//!
//! Used by the e2e/Pulse test harness to pre-seed `crates/nmp-testing/fixtures/test_nsec.txt`.
//!
//! ```bash
//! cargo run -q -p nmp-signers --example gen_nsec
//! ```

use nostr::nips::nip19::ToBech32;
use nostr::Keys;

fn main() {
    let keys = Keys::generate();
    let nsec = keys
        .secret_key()
        .to_bech32()
        .expect("bech32 encode of generated secret");
    print!("{}", nsec);
}
