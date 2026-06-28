//! Runnable headless demo for the login → following-timeline → live-update
//! worked example.
//!
//! ```sh
//! cargo run -p nmp-example-login-timeline --features harness --example login_timeline
//! ```
//!
//! It signs in with a throwaway nsec, follows a freshly-generated author, and
//! renders the following timeline from the Rust-owned `nmp.feed.home` typed
//! projection — first after the author's initial note, then again after a live
//! second note arrives. The host code here is rendering + login intent only;
//! every line of relay routing, subscription lifecycle, cache invalidation, and
//! replaceable-event policy lives in NMP.

use nmp_example_login_timeline::harness::run_demo;
use nmp_example_login_timeline::short_pubkey;

fn main() {
    let result = run_demo();

    println!(
        "Signed in. Following author {}",
        short_pubkey(&result.followed_author)
    );
    println!();
    println!(
        "Following timeline after login ({} row(s)):",
        result.after_login.len()
    );
    for row in &result.after_login {
        println!("  {}", row.render_line());
    }

    println!();
    println!(
        "Following timeline after a LIVE update ({} row(s)):",
        result.after_live_update.len()
    );
    for row in &result.after_live_update {
        println!("  {}", row.render_line());
    }

    assert!(
        !result.after_login.is_empty(),
        "the following timeline must render at least one row after login"
    );
    assert!(
        result.after_live_update.len() > result.after_login.len(),
        "a live note from the followed author must add a row"
    );
    println!();
    println!(
        "OK — login → following-timeline render → live update, zero relay/sub code in the shell."
    );
}
