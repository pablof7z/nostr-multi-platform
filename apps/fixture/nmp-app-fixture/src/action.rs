/// `AppAction` — the top-level action discriminant for the fixture app.
///
/// The `Nip29PublishPlan` variant proves at compile time that `PublishPlan` and
/// `RelayPin` (from `nmp-nip29`) are reachable through the per-app FFI surface.
/// Per ADR-0012/0013: the publish planner routes via `RelayPin`; the FFI layer
/// serialises the `PublishPlan` and hands it to the signer bridge.
///
/// This wiring is the FFI/codegen build verification deliverable for M11.5 T56:
/// `cargo check -p nmp-app-fixture` proves the types compile end-to-end.
#[derive(Clone, Debug, PartialEq)]
pub enum AppAction {
    Kernel(nmp_core::KernelAction),
    FixtureTodoCore(fixture_todo_core::Action),
    /// A pre-signing publish plan whose `pin_to: Some(RelayPin)` will be
    /// dispatched to the signer bridge and then routed to `RelayPin.relay_url`.
    Nip29PublishPlan(nmp_nip29::action::PublishPlan),
}

impl AppAction {
    pub fn namespace(&self) -> &'static str {
        match self {
            Self::Kernel(_) => "kernel",
            Self::FixtureTodoCore(_) => "fixture-todo-core",
            Self::Nip29PublishPlan(_) => "nip29",
        }
    }
}
