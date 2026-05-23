#[derive(Clone, Debug, PartialEq)]
pub enum AppAction {
    Kernel(nmp_core::KernelAction),
    FixtureTodoCore(fixture_todo_core::Action),
}

impl AppAction {
    #[must_use] 
    pub fn namespace(&self) -> &'static str {
        match self {
            Self::Kernel(_) => "kernel",
            Self::FixtureTodoCore(_) => "fixture-todo-core",
        }
    }
}
