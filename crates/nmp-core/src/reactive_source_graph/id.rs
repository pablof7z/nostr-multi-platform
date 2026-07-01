use std::fmt;

/// Owner/lifetime bucket for source-graph nodes.
///
/// A scope is usually a read session, component claim, or kernel-owned runtime
/// owner. Closing the scope is expected to tear down graph nodes and their
/// downstream effects in a later integration layer.
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct GraphScopeId(String);

impl GraphScopeId {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for GraphScopeId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for GraphScopeId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl fmt::Debug for GraphScopeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("GraphScopeId").field(&self.0).finish()
    }
}

impl fmt::Display for GraphScopeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Stable id for one source-graph input, derived value, or effect node.
#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct SourceNodeId(String);

impl SourceNodeId {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<&str> for SourceNodeId {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for SourceNodeId {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl fmt::Debug for SourceNodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("SourceNodeId").field(&self.0).finish()
    }
}

impl fmt::Display for SourceNodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Monotonic per-node revision.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Ord, PartialOrd)]
pub struct SourceNodeRevision(u64);

impl SourceNodeRevision {
    #[must_use]
    pub fn get(self) -> u64 {
        self.0
    }

    pub(crate) fn bump(&mut self) {
        self.0 = self.0.saturating_add(1);
    }
}
