//! A6 negative fixture — these shapes must NOT be flagged by rule A6.

/// Using `register_typed_snapshot_projection` is the correct typed lane — not a violation.
pub fn install_typed_projection(app: &mut FakeApp) {
    app.register_typed_snapshot_projection("wallet", || None::<TypedProjectionData>);
}

/// `TypedProjectionFn` is the correct type — not a violation.
pub fn typed_fn_type() {
    let _f: TypedProjectionFn = Box::new(|| None);
}

/// `TickObserverFn` is unrelated to the generic lane — not a violation.
pub fn tick_observer_type() {
    let _f: TickObserverFn = Box::new(|| {});
}

/// A doc comment mentioning the banned symbols is not a call — exempt.
///
/// The old `register_snapshot_projection` seam has been deleted. The old
/// `ProjectionFn` type is gone. Use `register_typed_snapshot_projection`
/// and `TypedProjectionFn` instead.
pub fn documented_correctly() {}

/// String literals containing banned tokens for documentation are NOT violations.
pub fn string_reference() -> &'static str {
    "use register_typed_snapshot_projection instead of register_snapshot_projection"
}

pub struct FakeApp;
pub struct TypedProjectionData;
pub type TypedProjectionFn = Box<dyn Fn() -> Option<TypedProjectionData>>;
pub type TickObserverFn = Box<dyn Fn()>;

impl FakeApp {
    pub fn register_typed_snapshot_projection(
        &mut self,
        _key: &str,
        _f: impl Fn() -> Option<TypedProjectionData>,
    ) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Inside a cfg(test) block the banned tokens are exempt.
    #[test]
    fn test_cfg_is_exempt() {
        // Calls to register_snapshot_projection inside test cfg are exempt.
        struct FakeOldApp;
        impl FakeOldApp {
            fn register_snapshot_projection(&mut self, _k: &str, _f: fn() -> ()) {}
        }
        let mut old = FakeOldApp;
        old.register_snapshot_projection("key", || {});
    }
}
