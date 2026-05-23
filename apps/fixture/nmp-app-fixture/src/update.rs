#[derive(Clone, Debug, PartialEq)]
pub enum AppUpdate {
    Kernel(nmp_core::KernelUpdate),
    FixtureTodoCore(fixture_todo_core::Update),
}

impl AppUpdate {
    #[must_use] 
    pub fn namespace(&self) -> &'static str {
        match self {
            Self::Kernel(_) => "kernel",
            Self::FixtureTodoCore(_) => "fixture-todo-core",
        }
    }
}
