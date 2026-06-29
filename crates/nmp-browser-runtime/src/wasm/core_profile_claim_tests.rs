use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use super::*;

#[test]
fn warm_profile_resolve_acknowledges_without_snapshot_push() {
    let mut core = NmpRuntimeCore::new();

    let pushes = Arc::new(AtomicUsize::new(0));
    let pushes2 = Arc::clone(&pushes);
    core.set_snapshot_sink(Some(Box::new(move |_bytes| {
        pushes2.fetch_add(1, Ordering::SeqCst);
    })));

    let _ = core.handle_json_request(&start_req());
    core.push_snapshot_bytes_if_sink();
    let baseline = pushes.load(Ordering::SeqCst);
    assert!(baseline > 0, "start must produce the baseline snapshot");

    let first = core.handle_json_request(&resolve_profile_ref_req("resolve-1"));
    assert!(first.contains("action_accepted"), "first={first}");
    core.push_snapshot_bytes_if_sink();
    let after_first = pushes.load(Ordering::SeqCst);
    assert_eq!(
        after_first,
        baseline + 1,
        "first profile resolve mutates the kernel and pushes one snapshot"
    );

    let second = core.handle_json_request(&resolve_profile_ref_req("resolve-2"));
    assert!(second.contains("action_accepted"), "second={second}");
    core.push_snapshot_bytes_if_sink();
    assert_eq!(
        pushes.load(Ordering::SeqCst),
        after_first,
        "warm identical profile resolve must not push a redundant snapshot"
    );
}

#[test]
fn noop_profile_release_acknowledges_without_snapshot_push() {
    let mut core = NmpRuntimeCore::new();

    let pushes = Arc::new(AtomicUsize::new(0));
    let pushes2 = Arc::clone(&pushes);
    core.set_snapshot_sink(Some(Box::new(move |_bytes| {
        pushes2.fetch_add(1, Ordering::SeqCst);
    })));

    let _ = core.handle_json_request(&start_req());
    core.push_snapshot_bytes_if_sink();
    let _ = core.handle_json_request(&resolve_profile_ref_req("resolve-1"));
    core.push_snapshot_bytes_if_sink();
    let after_resolve = pushes.load(Ordering::SeqCst);

    let first = core.handle_json_request(&release_profile_ref_req("release-1"));
    assert!(first.contains("action_accepted"), "first={first}");
    core.push_snapshot_bytes_if_sink();
    let after_first_release = pushes.load(Ordering::SeqCst);
    assert_eq!(
        after_first_release,
        after_resolve + 1,
        "first profile release mutates the kernel and pushes one snapshot"
    );

    let second = core.handle_json_request(&release_profile_ref_req("release-2"));
    assert!(second.contains("action_accepted"), "second={second}");
    core.push_snapshot_bytes_if_sink();
    assert_eq!(
        pushes.load(Ordering::SeqCst),
        after_first_release,
        "second release of the same profile consumer must not push a redundant snapshot"
    );
}
