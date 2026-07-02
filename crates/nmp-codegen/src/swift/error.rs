//! Owns `SwiftEmitError` — the error type shared by every Swift-emission
//! entry point in [`crate::swift`] (`render_swift`, `generate_swift`,
//! `check_swift`) and by the Stage 1 flat-record emitter in
//! [`crate::swift::flat_record_emit`].
//!
//! Split out of `swift.rs` so the top-level orchestration file stays under
//! the file-size ceiling; the type itself is re-exported at `crate::swift::
//! SwiftEmitError` (and from there at the crate root via `nmp_codegen::
//! SwiftEmitError`) so no caller-visible path changes.

/// What went wrong during Swift emission. Carries enough context that a
/// regression in Stage 1 (Rust type took on a non-flat field shape) names
/// the offending Swift type and Rust path.
///
/// Keeps `nmp-codegen` dependency-free of `thiserror` to match the existing
/// crate posture (every other module uses `String` errors). The hand-rolled
/// `Display` + `Error` impls below give Stage 1 callers `?` propagation
/// without dragging in a new dep tree.
#[derive(Debug)]
pub enum SwiftEmitError {
    /// The input JSON did not decode as a stream of [`ProjectionSchemaDocument`] values.
    ParseFailed { reason: String },
    /// The schema document version doesn't match the emitter's supported
    /// set. Bump emitter + document together when this trips.
    UnsupportedDocumentVersion { found: u32, expected: u32 },
    /// Two schema-owner documents tried to emit the same Swift type.
    DuplicateSwiftType { swift_name: String },
    /// One pilot type's schema isn't a flat object — Stage 1 deliberately
    /// rejects this so the dotted-key / tagged-enum work goes through
    /// Stage 2 / 3 instead of being silently emitted wrong here.
    Unsupported {
        swift_name: String,
        rust_path: String,
        reason: String,
    },
    /// Filesystem operations behind `generate_swift` / `check_swift`.
    Io(std::io::Error),
}

impl std::fmt::Display for SwiftEmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ParseFailed { reason } => {
                write!(
                    f,
                    "failed to parse projection schema document stream: {reason}"
                )
            }
            Self::UnsupportedDocumentVersion { found, expected } => write!(
                f,
                "projection schema document version {found} unsupported by this nmp-codegen \
                 build (expected version {expected}). Regenerate by piping the schema-owner \
                 dump binaries into `nmp-codegen gen swift`."
            ),
            Self::DuplicateSwiftType { swift_name } => write!(
                f,
                "duplicate Swift schema type `{swift_name}` across projection schema documents"
            ),
            Self::Unsupported {
                swift_name,
                rust_path,
                reason,
            } => write!(
                f,
                "cannot emit Swift for `{swift_name}` ({rust_path}): {reason}. \
                 Stage 1 only supports flat-record schemas; tagged enums and \
                 nested registries are Stage 2/3 scope per \
                 docs/retired/codegen-v6.md."
            ),
            Self::Io(err) => write!(f, "io: {err}"),
        }
    }
}
impl std::error::Error for SwiftEmitError {}
impl From<std::io::Error> for SwiftEmitError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}
