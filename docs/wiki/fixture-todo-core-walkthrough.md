---
title: Fixture Todo-Core Walkthrough — Actual Crate Conventions
slug: fixture-todo-core-walkthrough
summary: The fixture-todo-core walkthrough must reflect the real crate, which uses Arc<Mutex<Vec<TodoRecord>>> as its store, exports pub const ACTION_NAMESPACE and pub t
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-30
updated: 2026-05-30
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:c3f757f1-6292-4e52-b520-5bb52e7de2bf
---

# Fixture Todo-Core Walkthrough — Actual Crate Conventions

## Core Conventions

The fixture-todo-core walkthrough must reflect the real crate, which uses Arc<Mutex<Vec<TodoRecord>>> as its store, exports pub const ACTION_NAMESPACE and pub type Store as codegen conventions, and has apply_todo_action() as a plain function rather than a kernel-driven step machine. [^c3f75-10]


## Walkthrough Scope

The microblog-core crate referenced in §19b is a hypothetical walkthrough-only crate, not a real fixture in the codebase. [^c3f75-11]
## See Also

