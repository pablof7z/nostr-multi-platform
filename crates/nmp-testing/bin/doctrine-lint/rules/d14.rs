//! D14 — typed snapshot projection slots (PR-I).
//!
//! Three actor-owned relay-shaped caches sat behind bare
//! `Arc<Mutex<Vec<…>>>` slots before PR-I. The bare-`Vec` shape hid the
//! slot's snapshot-projection purpose, so future regressions added more of
//! them under reviewer noise. D14 catches the field-declaration shape and
//! steers callers to the typed-slot wrappers in
//! `crates/nmp-core/src/kernel/relay_projection.rs`.
//!
//! ## What this catches
//!
//! `name: Arc<Mutex<Vec<...>>>` (or `Option<…>`) as a **field declaration**
//! inside a struct named `NmpApp`, `Kernel`, `Nip65OutboxResolver` (PR-I2),
//! or one starting with `Actor` — under `crates/nmp-core/src/`.
//!
//! ## Escape hatches
//!
//! - Promote to a typed slot wrapper (preferred — see `relay_projection.rs`).
//! - Out-of-scope structs (`PublishEngine`, NIP-crate types, …) tolerated.
//! - Tests / fixtures / `#[cfg(test)]` blocks (walker exemptions).
//! - Per-line `// doctrine-allow: D14 — reason`.

use std::path::Path;

pub const ID: &str = "D14";

/// True iff the file lives under `crates/nmp-core/src/`. Other crates and
/// the doctrine-lint binary's own source tree are out of scope: the rule
/// disciplines the kernel/actor substrate, not app code or test
/// infrastructure.
pub fn file_in_scope(path: &Path) -> bool {
    let s = path.to_string_lossy().replace('\\', "/");
    // Strictly scoped to nmp-core's `src/` tree. `--path` invocations that
    // point at a fixture dir bypass this gate via `--d14-extra-scope`.
    let in_nmp_core_src = s.contains("/crates/nmp-core/src/")
        || s.starts_with("crates/nmp-core/src/");
    if !in_nmp_core_src {
        return false;
    }
    // Exempt the doctrine-lint binary's source tree (its fixture files for
    // OTHER rules legitimately mention `Arc<Mutex<Vec<…>>>` strings).
    if s.contains("/bin/doctrine-lint/") {
        return false;
    }
    true
}

/// Detect a struct field declaration whose type is `Arc<Mutex<Vec<…>>>` (or
/// `Option<Arc<Mutex<Vec<…>>>>`) AND whose enclosing struct is `NmpApp`,
/// `Kernel`, or starts with `Actor`. Emit a finding when both hold.
///
/// The struct-name match is line-stateful: the caller passes the most recent
/// `struct X ... {` name seen by the walker (or `None` outside any struct).
/// `crates/nmp-testing/bin/doctrine-lint/rules/d14.rs` callers track this
/// via [`StructTracker`] (defined below).
pub fn check(
    line: &str,
    is_comment: bool,
    enclosing_struct: Option<&str>,
) -> Vec<(usize, String, String)> {
    if is_comment {
        return Vec::new();
    }
    let Some(struct_name) = enclosing_struct else {
        return Vec::new();
    };
    if !is_in_scope_struct(struct_name) {
        return Vec::new();
    }
    let Some((col, snippet)) = parse_arc_mutex_vec_field(line) else {
        return Vec::new();
    };
    vec![(
        col,
        format!(
            "field `{}` declared as `Arc<Mutex<Vec<…>>>` on `{}` — D14 \
             requires a typed snapshot-projection slot (see \
             `kernel/relay_projection.rs`)",
            snippet, struct_name
        ),
        format!(
            "introduce a typed newtype wrapper (e.g. `pub struct \
             {}Slot(pub(crate) Vec<…>); pub type ...Slot = \
             Arc<Mutex<...>>;`) and declare the field as the named slot \
             type instead",
            snippet
        ),
    )]
}

/// True iff `name` is `NmpApp`, `Kernel`, `Nip65OutboxResolver`, or starts
/// with `Actor`. Explicit name-list (PR-I2 added the resolver since the
/// original bare slots originated there). Add future actor-owned
/// publish/relay structs to the list explicitly; a wildcard like
/// `ends_with("Resolver")` would catch unrelated types.
fn is_in_scope_struct(name: &str) -> bool {
    name == "NmpApp"
        || name == "Kernel"
        || name == "Nip65OutboxResolver"
        || name.starts_with("Actor")
}

/// Parse a struct-field-declaration line. Returns the (column-1-indexed,
/// field-name-snippet) if the line declares a field whose type is
/// `Arc<Mutex<Vec<…>>>` or `Option<Arc<Mutex<Vec<…>>>>`. Returns `None` for
/// any other line.
///
/// Recognised shapes (whitespace-tolerant, optional `pub`/`pub(crate)`):
///
/// - `    name: Arc<Mutex<Vec<X>>>,`
/// - `    pub name: Arc<Mutex<Vec<X>>>,`
/// - `    pub(crate) name: Option<Arc<Mutex<Vec<X>>>>,`
fn parse_arc_mutex_vec_field(line: &str) -> Option<(usize, String)> {
    let arc_pos = line.find("Arc<Mutex<Vec<")?;
    // Disqualify obvious non-field-decls: `let` / `fn` / `type` / `static` /
    // `const` / `where`. A struct field starts with optional visibility,
    // optional attribute, then identifier.
    let trimmed = line.trim_start();
    for prefix in ["let ", "fn ", "type ", "static ", "const ", "pub fn ", "where "] {
        if trimmed.starts_with(prefix) {
            return None;
        }
    }
    let colon_pos = line[..arc_pos].rfind(':')?;
    let before_colon = line[..colon_pos].trim_end();
    let field_name = before_colon
        .rsplit(|c: char| c.is_whitespace() || c == '(' || c == ')')
        .find(|tok| !tok.is_empty())
        .unwrap_or("<field>")
        .to_string();
    if matches!(field_name.as_str(), "type" | "let" | "fn" | "where")
        || !is_identifier(&field_name)
    {
        return None;
    }
    Some((arc_pos + 1, field_name)) // 1-indexed column
}

/// True iff `s` is a Rust identifier: `[a-zA-Z_][a-zA-Z0-9_]*`. Used by
/// [`parse_arc_mutex_vec_field`] to reject false positives whose pre-colon
/// token is not a field name (e.g. a path segment like `::SomeType`).
fn is_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Track the most recent `struct <Name>` declaration the walker has seen,
/// so [`check`] knows which struct the current line's field is in.
///
/// Not a full parser — it counts braces inside `count_braces_ignoring_strings`
/// the way the D8 hot-path tracker does. Accuracy bar: "zero false positives
/// on current `nmp-core/`," not AST-correct on adversarial input.
#[derive(Default)]
pub struct StructTracker {
    /// Stack of `(struct_name, brace_depth_at_open)` for nested-or-sibling
    /// struct definitions. A field line is in the *innermost* open struct,
    /// which is the top of the stack.
    open: Vec<(String, i32)>,
    /// Running brace depth across the file.
    depth: i32,
}

impl StructTracker {
    /// Observe one line. Updates the brace-depth bookkeeping and the open-
    /// struct stack. MUST be called for every line in the file, in order,
    /// before [`Self::current_struct`] is read for that line.
    ///
    /// `is_comment` lets the caller pass through the walker's comment flag
    /// so block-comment-internal lines do not move the brace counter.
    pub fn observe_line(&mut self, line: &str, is_comment: bool) {
        // Pop any structs whose closing `}` ran out on the *previous* line.
        // We do this at the top so a line that opens a new struct still
        // sees the post-close depth.
        self.pop_closed();
        if is_comment {
            return;
        }
        if let Some(name) = parse_struct_open(line) {
            // The opening brace of the struct is consumed below by the
            // brace-delta accounting, so record the depth *before* applying it.
            self.open.push((name, self.depth));
        }
        let (opens, closes) = crate::braces::count_braces_ignoring_strings(line);
        self.depth += opens as i32 - closes as i32;
        // Pop again after the brace-delta is applied so a struct that
        // opens and closes on the same line (e.g. a unit struct
        // followed by a `}` on the same physical line) settles cleanly,
        // and so the `current_struct()` query returns `None` immediately
        // after the closing `}` line is observed — without needing one
        // more `observe_line` call to flush the stack.
        self.pop_closed();
    }

    /// Pop every struct on the stack whose recorded opened-at depth is
    /// strictly greater than (or equal to) the current depth. A struct
    /// with `opened_at = N` closes when depth returns to `N` (the
    /// matching `}` brings it back to its pre-opening level).
    fn pop_closed(&mut self) {
        while let Some((_, opened_at)) = self.open.last() {
            if self.depth <= *opened_at {
                self.open.pop();
            } else {
                break;
            }
        }
    }

    /// Return the name of the innermost open struct (`None` if the current
    /// line is outside any struct). Read AFTER `observe_line` for the same
    /// line.
    pub fn current_struct(&self) -> Option<&str> {
        self.open.last().map(|(n, _)| n.as_str())
    }
}

/// Parse `pub struct Foo {` / `struct Foo {` / `struct Foo<T> {` style
/// lines, returning the struct name. Returns `None` for any other line
/// (including `impl`, `trait`, `enum`, type aliases, and `fn`).
fn parse_struct_open(line: &str) -> Option<String> {
    let trimmed = line.trim_start();
    // Strip visibility prefixes — `pub`, `pub(crate)`, `pub(super)`,
    // `pub(in crate::path)`. The body after the prefix must begin with
    // `struct `.
    let after_vis = strip_visibility(trimmed);
    let rest = after_vis.strip_prefix("struct ")?;
    // The identifier is the first token, stopping at whitespace, `<`, `{`,
    // `(`, or `;` (the unit-struct case `struct Foo;`).
    let stop = rest
        .find(|c: char| c.is_whitespace() || c == '<' || c == '{' || c == '(' || c == ';')
        .unwrap_or(rest.len());
    let name = &rest[..stop];
    if name.is_empty() {
        return None;
    }
    Some(name.to_string())
}

/// Strip an optional `pub`, `pub(crate)`, `pub(super)`, or `pub(in path)`
/// prefix from `s` (plus the trailing whitespace). Returns the unchanged
/// `s` when nothing matches — visibility is optional.
fn strip_visibility(s: &str) -> &str {
    let after_pub = s.strip_prefix("pub").unwrap_or(s);
    if after_pub.is_empty() {
        return after_pub.trim_start();
    }
    // Match `pub(...)` exactly (no inner spaces in the paren group).
    if let Some(rest) = after_pub.strip_prefix('(') {
        if let Some(idx) = rest.find(')') {
            return rest[idx + 1..].trim_start();
        }
    }
    // `pub` followed by whitespace.
    if let Some(rest) = after_pub.strip_prefix(' ') {
        return rest.trim_start();
    }
    if let Some(rest) = after_pub.strip_prefix('\t') {
        return rest.trim_start();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn flags_bare_arc_mutex_vec_field_on_kernel() {
        // Canonical positive — a `Kernel` field declared with the bare
        // `Arc<Mutex<Vec<…>>>` pattern must fire.
        let hits = check(
            "    indexer_relays: Arc<Mutex<Vec<String>>>,",
            false,
            Some("Kernel"),
        );
        assert_eq!(hits.len(), 1, "expected exactly one D14 finding");
        assert!(
            hits[0].1.contains("D14"),
            "message must mention the rule id; got: {}",
            hits[0].1
        );
        assert!(
            hits[0].1.contains("indexer_relays"),
            "message must name the offending field; got: {}",
            hits[0].1
        );
        assert!(
            hits[0].1.contains("Kernel"),
            "message must name the enclosing struct; got: {}",
            hits[0].1
        );
    }

    #[test]
    fn flags_bare_arc_mutex_vec_field_on_nmp_app() {
        // Same shape as the `Kernel` case but on the FFI handle struct.
        let hits = check(
            "    relay_edit_rows: Arc<Mutex<Vec<RelayEditRow>>>,",
            false,
            Some("NmpApp"),
        );
        assert_eq!(hits.len(), 1);
        assert!(hits[0].1.contains("NmpApp"));
        assert!(hits[0].1.contains("relay_edit_rows"));
    }

    #[test]
    fn flags_bare_arc_mutex_vec_field_on_actor_runtime_struct() {
        // Any struct whose name starts with `Actor` is in scope (matches the
        // actor-runtime family — `ActorRuntime`, `ActorContext`, …).
        let hits = check(
            "    pending: Arc<Mutex<Vec<u32>>>,",
            false,
            Some("ActorRuntime"),
        );
        assert_eq!(hits.len(), 1);
        assert!(hits[0].1.contains("ActorRuntime"));
    }

    #[test]
    fn allows_typed_slot_field() {
        // The escape hatch: a typed wrapper around the Vec means the bare
        // `Arc<Mutex<Vec<…>>>` substring does not appear on the field line.
        let hits = check(
            "    indexer_relays: IndexerRelaysSlot,",
            false,
            Some("Kernel"),
        );
        assert!(hits.is_empty(), "typed slot field must not fire");
    }

    #[test]
    fn allows_arc_mutex_vec_field_on_unrelated_struct() {
        // Out-of-scope struct names must not trigger — D14 disciplines
        // exactly the kernel/actor/FFI substrate identifiers.
        let hits = check(
            "    pending: Arc<Mutex<Vec<u32>>>,",
            false,
            Some("PublishEngine"),
        );
        assert!(hits.is_empty(), "non-Kernel/NmpApp/Actor struct must not fire");
    }

    #[test]
    fn ignores_comment_line() {
        // A doc-comment containing the offending pattern must not fire (the
        // walker would also pass `is_comment = true` here; assert defensively).
        let hits = check(
            "    /// historical: indexer_relays: Arc<Mutex<Vec<String>>>",
            true,
            Some("Kernel"),
        );
        assert!(hits.is_empty());
    }

    #[test]
    fn ignores_let_binding_with_same_type() {
        // A `let` binding inside a method body sometimes uses the same
        // `Arc<Mutex<Vec<…>>>` type (e.g. constructing a slot). D14 fires
        // only on struct *field* declarations.
        let hits = check(
            "    let pending: Arc<Mutex<Vec<u32>>> = Arc::new(Mutex::new(Vec::new()));",
            false,
            Some("Kernel"),
        );
        assert!(
            hits.is_empty(),
            "`let` bindings must not fire — D14 is a field-declaration rule"
        );
    }

    #[test]
    fn ignores_function_signature_with_same_type() {
        // A function argument with this type (e.g. a public constructor
        // accepting a shared slot) must not fire either.
        let hits = check(
            "    pub fn new(slot: Arc<Mutex<Vec<String>>>) -> Self {",
            false,
            Some("Kernel"),
        );
        assert!(
            hits.is_empty(),
            "function-argument types must not fire — D14 is a field-declaration rule"
        );
    }

    #[test]
    fn flags_option_wrapped_arc_mutex_vec_field() {
        // The handle slot is sometimes `Option<Arc<Mutex<Vec<…>>>>` — the
        // `Arc<Mutex<Vec<` substring still appears on the line, so the rule
        // catches it.
        let hits = check(
            "    relay_edit_rows_handle: Option<Arc<Mutex<Vec<RelayEditRow>>>>,",
            false,
            Some("Kernel"),
        );
        assert_eq!(hits.len(), 1, "Option<Arc<Mutex<Vec<…>>>> must fire");
        assert!(hits[0].1.contains("relay_edit_rows_handle"));
    }

    #[test]
    fn struct_tracker_resolves_innermost_struct_name() {
        // Walk a small synthetic file and confirm the tracker reports the
        // right enclosing struct on a field line.
        let mut tracker = StructTracker::default();
        for line in [
            "pub struct Kernel {",
            "    indexer_relays: Arc<Mutex<Vec<String>>>,",
            "}",
            "pub struct PublishEngine {",
            "    pending: Arc<Mutex<Vec<u32>>>,",
            "}",
        ] {
            tracker.observe_line(line, false);
            // Capture the result after each line for the assertions below.
            if line.trim_start().starts_with("indexer_relays") {
                assert_eq!(tracker.current_struct(), Some("Kernel"));
            }
            if line.trim_start().starts_with("pending:") {
                assert_eq!(tracker.current_struct(), Some("PublishEngine"));
            }
        }
        // After the second `}`, we are outside both structs.
        assert!(tracker.current_struct().is_none());
    }

    #[test]
    fn struct_tracker_skips_struct_open_inside_block_comment() {
        // A `struct Foo {` inside a block-comment'd block must not register —
        // the walker passes `is_comment = true` for such lines and the
        // tracker must respect it.
        let mut tracker = StructTracker::default();
        tracker.observe_line("/* pub struct Fake {", true);
        // The opening brace of the fake struct is inside the block comment
        // so the tracker must NOT push it. Note: we explicitly pass true
        // for `is_comment` so block-comment-internal lines do not move
        // the brace counter or open-struct stack.
        assert!(tracker.current_struct().is_none());
    }

    #[test]
    fn parse_struct_open_handles_visibility_and_generics() {
        assert_eq!(parse_struct_open("pub struct Foo {").as_deref(), Some("Foo"));
        assert_eq!(parse_struct_open("struct Bar<T> {").as_deref(), Some("Bar"));
        assert_eq!(
            parse_struct_open("pub(crate) struct Baz {").as_deref(),
            Some("Baz")
        );
        assert_eq!(parse_struct_open("    struct Qux;").as_deref(), Some("Qux"));
        // Negative cases — not a struct definition.
        assert_eq!(parse_struct_open("impl Foo {").as_deref(), None);
        assert_eq!(parse_struct_open("trait Foo {").as_deref(), None);
        assert_eq!(parse_struct_open("fn foo() {").as_deref(), None);
        assert_eq!(parse_struct_open("let x = struct;").as_deref(), None);
    }

    #[test]
    fn file_in_scope_includes_nmp_core_src() {
        assert!(file_in_scope(&PathBuf::from(
            "crates/nmp-core/src/kernel/mod.rs"
        )));
        assert!(file_in_scope(&PathBuf::from(
            "/abs/path/crates/nmp-core/src/ffi/mod.rs"
        )));
    }

    #[test]
    fn file_in_scope_excludes_other_crates() {
        // Other protocol crates are out of scope — D14 is substrate-only.
        assert!(!file_in_scope(&PathBuf::from(
            "crates/nmp-nip29/src/lib.rs"
        )));
        assert!(!file_in_scope(&PathBuf::from(
            "apps/chirp/nmp-app-chirp/src/ffi.rs"
        )));
        // nmp-testing's doctrine-lint fixtures legitimately reference the
        // banned pattern as text — they must be exempt.
        assert!(!file_in_scope(&PathBuf::from(
            "crates/nmp-testing/bin/doctrine-lint/fixtures/d14/pos.rs"
        )));
    }
}
