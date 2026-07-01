//! Declared framework-surface tokens.
//!
//! Framework-owned action namespaces, projection keys, and schema identities are
//! protected surfaces: production APIs should receive a declared token instead
//! of accepting a raw string. Dynamic app projection keys remain possible, but
//! they travel through [`DynamicProjectionKey`] so an app-owned path is explicit.

/// Compile-time token for an NMP-owned action namespace.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeclaredActionNamespace {
    value: &'static str,
    owner_claim: &'static str,
}

impl DeclaredActionNamespace {
    /// Declare a framework-owned namespace and cite the ownership claim that
    /// owns it. `nmp crate-ownership audit --deny` verifies the cited value
    /// appears in the action contract and ownership descriptor set.
    #[must_use]
    pub const fn framework(value: &'static str, owner_claim: &'static str) -> Self {
        require_non_empty(owner_claim, "declared action namespace owner claim");
        Self { value, owner_claim }
    }

    /// Declare an app-owned namespace. This path is not valid for `nmp.*`
    /// framework namespaces.
    #[must_use]
    pub const fn app_owned(value: &'static str) -> Self {
        reject_framework_prefix(value, "app-owned action namespace");
        Self {
            value,
            owner_claim: "",
        }
    }

    /// The routing namespace carried on dispatch envelopes.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.value
    }

    /// The cited ownership claim for framework-owned namespaces.
    #[must_use]
    pub const fn owner_claim(self) -> &'static str {
        self.owner_claim
    }
}

impl PartialEq<&str> for DeclaredActionNamespace {
    fn eq(&self, other: &&str) -> bool {
        self.value == *other
    }
}

impl PartialEq<DeclaredActionNamespace> for &str {
    fn eq(&self, other: &DeclaredActionNamespace) -> bool {
        *self == other.value
    }
}

/// Compile-time token for an NMP-owned snapshot projection key.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeclaredProjectionKey {
    value: &'static str,
    owner_claim: &'static str,
}

impl DeclaredProjectionKey {
    /// Declare a framework-owned projection key and cite the ownership claim
    /// that owns it. The crate-ownership audit verifies the key and claim
    /// against the projection contract.
    #[must_use]
    pub const fn framework(value: &'static str, owner_claim: &'static str) -> Self {
        require_non_empty(owner_claim, "declared projection key owner claim");
        Self { value, owner_claim }
    }

    /// The snapshot projection key.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.value
    }

    /// The cited ownership claim.
    #[must_use]
    pub const fn owner_claim(self) -> &'static str {
        self.owner_claim
    }
}

impl PartialEq<&str> for DeclaredProjectionKey {
    fn eq(&self, other: &&str) -> bool {
        self.value == *other
    }
}

impl PartialEq<DeclaredProjectionKey> for &str {
    fn eq(&self, other: &DeclaredProjectionKey) -> bool {
        *self == other.value
    }
}

/// Runtime token for a framework-owned projection instance whose exact key is
/// assembled from a declared family plus a bounded runtime suffix.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrameworkProjectionKey {
    value: String,
    owner_claim: &'static str,
}

impl FrameworkProjectionKey {
    /// Build a declared framework projection instance and cite the family owner
    /// claim. Use this for framework-owned per-session keys such as
    /// `nmp.nip50.search.<session>`, not for app-owned feed keys.
    pub fn declared(
        value: impl Into<String>,
        owner_claim: &'static str,
    ) -> Result<Self, SurfaceTokenError> {
        let value = value.into();
        if !has_framework_prefix(&value) {
            return Err(SurfaceTokenError::MissingFrameworkPrefix {
                surface: "framework projection key",
                value,
            });
        }
        if owner_claim.trim().is_empty() {
            return Err(SurfaceTokenError::Empty {
                surface: "framework projection key owner claim",
            });
        }
        Ok(Self { value, owner_claim })
    }

    /// Borrow the projection key.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.value
    }

    /// The cited ownership claim for this projection family.
    #[must_use]
    pub const fn owner_claim(&self) -> &'static str {
        self.owner_claim
    }

    /// Consume into the owned projection key.
    #[must_use]
    pub fn into_string(self) -> String {
        self.value
    }
}

/// A typed snapshot-registration key.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProjectionRegistrationKey {
    /// Framework-owned projection surface.
    Framework(FrameworkProjectionKey),
    /// App-owned dynamic projection surface.
    Dynamic(DynamicProjectionKey),
}

impl ProjectionRegistrationKey {
    /// Borrow the registration key.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Framework(key) => key.as_str(),
            Self::Dynamic(key) => key.as_str(),
        }
    }

    /// Consume into the owned registration key.
    #[must_use]
    pub fn into_string(self) -> String {
        match self {
            Self::Framework(key) => key.into_string(),
            Self::Dynamic(key) => key.into_string(),
        }
    }
}

impl From<DeclaredProjectionKey> for ProjectionRegistrationKey {
    fn from(value: DeclaredProjectionKey) -> Self {
        Self::Framework(FrameworkProjectionKey {
            value: value.as_str().to_string(),
            owner_claim: value.owner_claim(),
        })
    }
}

impl From<FrameworkProjectionKey> for ProjectionRegistrationKey {
    fn from(value: FrameworkProjectionKey) -> Self {
        Self::Framework(value)
    }
}

impl From<DynamicProjectionKey> for ProjectionRegistrationKey {
    fn from(value: DynamicProjectionKey) -> Self {
        Self::Dynamic(value)
    }
}

/// Compile-time token for a declared framework schema identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DeclaredSchemaId {
    value: &'static str,
    owner_claim: &'static str,
}

impl DeclaredSchemaId {
    /// Declare a framework schema identity and cite the owning claim.
    #[must_use]
    pub const fn framework(value: &'static str, owner_claim: &'static str) -> Self {
        require_non_empty(owner_claim, "declared schema id owner claim");
        Self { value, owner_claim }
    }

    /// The stable schema identity.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        self.value
    }

    /// The cited ownership claim.
    #[must_use]
    pub const fn owner_claim(self) -> &'static str {
        self.owner_claim
    }
}

/// Runtime token for app-owned dynamic projection keys.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct DynamicProjectionKey {
    value: String,
}

impl DynamicProjectionKey {
    /// Build an app-owned dynamic projection key.
    ///
    /// `nmp.*` is reserved for declared framework surfaces; app/product feed
    /// keys should use an app-owned namespace such as `app.feed.*` or a
    /// product-owned prefix.
    pub fn app_owned(value: impl Into<String>) -> Result<Self, SurfaceTokenError> {
        let value = value.into();
        if has_framework_prefix(&value) {
            return Err(SurfaceTokenError::FrameworkPrefix {
                surface: "dynamic projection key",
                value,
            });
        }
        if value.trim().is_empty() {
            return Err(SurfaceTokenError::Empty {
                surface: "dynamic projection key",
            });
        }
        Ok(Self { value })
    }

    /// Borrow the projection key string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.value
    }

    /// Consume the token into the owned key string.
    #[must_use]
    pub fn into_string(self) -> String {
        self.value
    }
}

/// Failure constructing a dynamic/app-owned surface token.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SurfaceTokenError {
    /// The caller tried to use a reserved `nmp.*` framework prefix through an
    /// app-owned dynamic path.
    FrameworkPrefix {
        /// Surface being built.
        surface: &'static str,
        /// Rejected value.
        value: String,
    },
    /// The caller tried to declare a framework-owned token without the reserved
    /// `nmp.*` prefix.
    MissingFrameworkPrefix {
        /// Surface being built.
        surface: &'static str,
        /// Rejected value.
        value: String,
    },
    /// The token value was empty.
    Empty {
        /// Surface being built.
        surface: &'static str,
    },
}

impl core::fmt::Display for SurfaceTokenError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::FrameworkPrefix { surface, value } => write!(
                f,
                "{surface} {value:?} uses reserved framework prefix `nmp.`"
            ),
            Self::MissingFrameworkPrefix { surface, value } => write!(
                f,
                "{surface} {value:?} must use reserved framework prefix `nmp.`"
            ),
            Self::Empty { surface } => write!(f, "{surface} cannot be empty"),
        }
    }
}

impl std::error::Error for SurfaceTokenError {}

const fn require_non_empty(value: &'static str, surface: &'static str) {
    if value.is_empty() {
        let _ = surface;
        panic!("declared surface owner claim cannot be empty");
    }
}

const fn reject_framework_prefix(value: &'static str, surface: &'static str) {
    if has_framework_prefix(value) {
        let _ = surface;
        panic!("app-owned surface cannot use reserved framework prefix `nmp.`");
    }
}

const fn has_framework_prefix(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() >= 4 && bytes[0] == b'n' && bytes[1] == b'm' && bytes[2] == b'p' && bytes[3] == b'.'
}

#[cfg(test)]
mod tests {
    use super::*;

    const APP_ACTION: DeclaredActionNamespace =
        DeclaredActionNamespace::app_owned("app.example.action");
    const FW_ACTION: DeclaredActionNamespace =
        DeclaredActionNamespace::framework("nmp.example.action", "action.nmp.example.action");
    const FW_PROJECTION: DeclaredProjectionKey =
        DeclaredProjectionKey::framework("nmp.example.projection", "projection.nmp.example");

    #[test]
    fn declared_tokens_expose_values_and_claims() {
        assert_eq!(APP_ACTION.as_str(), "app.example.action");
        assert_eq!(APP_ACTION.owner_claim(), "");
        assert_eq!(FW_ACTION.as_str(), "nmp.example.action");
        assert_eq!(FW_ACTION.owner_claim(), "action.nmp.example.action");
        assert_eq!(FW_PROJECTION.as_str(), "nmp.example.projection");
        assert_eq!(FW_PROJECTION.owner_claim(), "projection.nmp.example");
    }

    #[test]
    fn dynamic_projection_rejects_framework_prefix() {
        assert!(DynamicProjectionKey::app_owned("test.feed.following").is_ok());
        assert!(matches!(
            DynamicProjectionKey::app_owned("nmp.feed.home"),
            Err(SurfaceTokenError::FrameworkPrefix { .. })
        ));
        assert!(matches!(
            DynamicProjectionKey::app_owned(""),
            Err(SurfaceTokenError::Empty { .. })
        ));
    }

    #[test]
    fn framework_projection_instance_requires_framework_prefix() {
        assert!(FrameworkProjectionKey::declared(
            "nmp.nip50.search.session",
            "projection.nmp.nip50.search"
        )
        .is_ok());
        assert!(matches!(
            FrameworkProjectionKey::declared("test.feed.following", "projection.nmp.bad"),
            Err(SurfaceTokenError::MissingFrameworkPrefix { .. })
        ));
    }
}
