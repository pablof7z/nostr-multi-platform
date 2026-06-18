//! A6 positive fixture — contains in-repo uses of the deleted schema-less
//! JSON snapshot-projection lane. The lint must flag each occurrence.

/// This represents hypothetical code that incorrectly tries to use the deleted
/// generic projection lane instead of the typed FlatBuffers sidecar.
pub fn install_bad_projection(app: &mut FakeApp) {
    // A6 violation: register_snapshot_projection is banned (generic lane deleted).
    app.register_snapshot_projection("wallet", || serde_json::Value::Null);
}

pub fn install_gated_projection(app: &mut FakeApp) {
    // A6 violation: register_snapshot_projection_gated is banned.
    app.register_snapshot_projection_gated("market", Arc::new(gate()), || serde_json::Value::Null);
}

pub fn uses_projection_fn_type() {
    // A6 violation: ProjectionFn type alias is banned.
    let _f: ProjectionFn = Box::new(|| serde_json::Value::Null);
}

pub fn uses_c_abi_symbol() {
    // A6 violation: nmp_app_register_snapshot_projection C symbol is banned.
    unsafe { nmp_app_register_snapshot_projection(std::ptr::null_mut(), std::ptr::null(), None); }
}

pub fn uses_registry_register() {
    // A6 violation: SnapshotRegistry::register is banned.
    SnapshotRegistry::register(&mut registry, "key", || serde_json::Value::Null);
}

pub fn uses_register_gated() {
    // A6 violation: .register_gated( is banned.
    registry.register_gated("key", gate, || serde_json::Value::Null);
}

// Stubs to make the fixture self-contained.
struct FakeApp;
struct Arc<T>(T);
fn gate() -> u64 { 0 }
struct ProjectionFn;
struct SnapshotRegistry;
let registry = SnapshotRegistry;
extern "C" { fn nmp_app_register_snapshot_projection(a: *mut (), b: *const i8, c: Option<fn()>); }
impl FakeApp {
    fn register_snapshot_projection(&mut self, _k: &str, _f: impl Fn() -> serde_json::Value) {}
    fn register_snapshot_projection_gated(&mut self, _k: &str, _g: Arc<u64>, _f: impl Fn() -> serde_json::Value) {}
}
