#[derive(Clone, Debug, PartialEq)]
pub enum ViewSpec {
    Kernel(nmp_core::KernelViewSpec),
    FixtureTodoCore(fixture_todo_core::ViewSpec),
}

impl ViewSpec {
    #[must_use] 
    pub fn namespace(&self) -> &'static str {
        match self {
            Self::Kernel(_) => "kernel",
            Self::FixtureTodoCore(_) => "fixture-todo-core",
        }
    }
}
