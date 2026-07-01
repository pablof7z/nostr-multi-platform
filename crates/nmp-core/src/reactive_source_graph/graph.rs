use std::collections::{BTreeMap, BTreeSet};

use super::id::{SourceNodeId, SourceNodeRevision};
use super::value::{boxed_value, SourceValue};

type BoxedReducer<E> =
    Box<dyn Fn(&GraphRead<'_, E>) -> Box<dyn SourceValue> + Send + Sync + 'static>;
type BoxedEffect<E> = Box<dyn Fn(&GraphRead<'_, E>) -> Option<E> + Send + Sync + 'static>;

/// Error raised while registering or propagating graph nodes.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum GraphError {
    DuplicateNode(SourceNodeId),
    MissingDependency {
        node: SourceNodeId,
        dependency: SourceNodeId,
    },
    MissingValue(SourceNodeId),
    NotInput(SourceNodeId),
    UnknownNode(SourceNodeId),
    ValueTypeMismatch {
        node: SourceNodeId,
        expected: &'static str,
        actual: &'static str,
    },
}

/// One typed input write for a graph turn.
pub struct SourceInputUpdate {
    pub(super) id: SourceNodeId,
    pub(super) value: Box<dyn SourceValue>,
}

impl SourceInputUpdate {
    #[must_use]
    pub fn new<T>(id: impl Into<SourceNodeId>, value: T) -> Self
    where
        T: Clone + Eq + Send + Sync + 'static,
    {
        Self {
            id: id.into(),
            value: boxed_value(value),
        }
    }

    #[must_use]
    pub fn id(&self) -> &SourceNodeId {
        &self.id
    }
}

/// Node revision recorded during one graph turn.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NodeChange {
    pub id: SourceNodeId,
    pub revision: SourceNodeRevision,
}

/// Result of one batched source-graph propagation turn.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GraphTurn<E> {
    pub(super) changed_nodes: Vec<NodeChange>,
    pub(super) effects: Vec<E>,
}

impl<E> GraphTurn<E> {
    #[must_use]
    pub fn changed_nodes(&self) -> &[NodeChange] {
        &self.changed_nodes
    }

    #[must_use]
    pub fn effects(&self) -> &[E] {
        &self.effects
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.changed_nodes.is_empty() && self.effects.is_empty()
    }

    #[must_use]
    pub fn into_effects(self) -> Vec<E> {
        self.effects
    }
}

/// Read-only typed view of current graph values.
pub struct GraphRead<'a, E> {
    pub(super) graph: &'a ReactiveSourceGraph<E>,
}

impl<E> GraphRead<'_, E> {
    #[must_use]
    pub fn get<T>(&self, id: &SourceNodeId) -> Option<&T>
    where
        T: 'static,
    {
        self.graph
            .nodes
            .get(id)
            .and_then(|node| node.value.as_ref())
            .and_then(|value| value.as_any().downcast_ref::<T>())
    }
}

pub(super) enum NodeKind<E> {
    Input,
    Derived {
        deps: Vec<SourceNodeId>,
        reducer: BoxedReducer<E>,
    },
    Effect {
        deps: Vec<SourceNodeId>,
        effect: BoxedEffect<E>,
    },
}

pub(super) struct SourceNode<E> {
    pub(super) revision: SourceNodeRevision,
    pub(super) value: Option<Box<dyn SourceValue>>,
    pub(super) kind: NodeKind<E>,
}

/// Actor-owned source dependency graph.
///
/// The graph is intentionally small: inputs carry typed values, derived nodes
/// recompute synchronously, and effect nodes emit typed internal effects after
/// a graph turn. It owns dependency propagation only; callers still own domain
/// reducers, acquisition primitives, projection registration, and teardown.
pub struct ReactiveSourceGraph<E> {
    pub(super) nodes: BTreeMap<SourceNodeId, SourceNode<E>>,
    pub(super) dependents: BTreeMap<SourceNodeId, BTreeSet<SourceNodeId>>,
}

impl<E> Default for ReactiveSourceGraph<E> {
    fn default() -> Self {
        Self {
            nodes: BTreeMap::new(),
            dependents: BTreeMap::new(),
        }
    }
}

impl<E> ReactiveSourceGraph<E> {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_input<T>(
        &mut self,
        id: impl Into<SourceNodeId>,
        value: T,
    ) -> Result<SourceNodeRevision, GraphError>
    where
        T: Clone + Eq + Send + Sync + 'static,
    {
        let id = id.into();
        self.ensure_new(&id)?;
        self.nodes.insert(
            id,
            SourceNode {
                revision: SourceNodeRevision::default(),
                value: Some(boxed_value(value)),
                kind: NodeKind::Input,
            },
        );
        Ok(SourceNodeRevision::default())
    }

    pub fn add_derived<T, R>(
        &mut self,
        id: impl Into<SourceNodeId>,
        deps: impl IntoIterator<Item = SourceNodeId>,
        reducer: R,
    ) -> Result<SourceNodeRevision, GraphError>
    where
        T: Clone + Eq + Send + Sync + 'static,
        R: Fn(&GraphRead<'_, E>) -> T + Send + Sync + 'static,
    {
        let id = id.into();
        let deps = deps.into_iter().collect::<Vec<_>>();
        self.validate_deps(&id, &deps)?;
        let initial = reducer(&self.read());
        let reducer = Box::new(move |read: &GraphRead<'_, E>| boxed_value(reducer(read)));
        self.insert_dependent_node(
            id,
            deps.clone(),
            SourceNode {
                revision: SourceNodeRevision::default(),
                value: Some(boxed_value(initial)),
                kind: NodeKind::Derived { deps, reducer },
            },
        );
        Ok(SourceNodeRevision::default())
    }

    pub fn add_effect<F>(
        &mut self,
        id: impl Into<SourceNodeId>,
        deps: impl IntoIterator<Item = SourceNodeId>,
        effect: F,
    ) -> Result<SourceNodeRevision, GraphError>
    where
        F: Fn(&GraphRead<'_, E>) -> Option<E> + Send + Sync + 'static,
    {
        let id = id.into();
        let deps = deps.into_iter().collect::<Vec<_>>();
        self.validate_deps(&id, &deps)?;
        self.insert_dependent_node(
            id,
            deps.clone(),
            SourceNode {
                revision: SourceNodeRevision::default(),
                value: None,
                kind: NodeKind::Effect {
                    deps,
                    effect: Box::new(effect),
                },
            },
        );
        Ok(SourceNodeRevision::default())
    }

    pub fn set_input<T>(
        &mut self,
        id: impl Into<SourceNodeId>,
        value: T,
    ) -> Result<GraphTurn<E>, GraphError>
    where
        T: Clone + Eq + Send + Sync + 'static,
    {
        self.apply_inputs([SourceInputUpdate::new(id, value)])
    }

    pub fn apply_inputs<I>(&mut self, updates: I) -> Result<GraphTurn<E>, GraphError>
    where
        I: IntoIterator<Item = SourceInputUpdate>,
    {
        let mut dirty = BTreeSet::new();
        let mut changed_nodes = Vec::new();
        for update in updates {
            self.apply_one_input(update, &mut dirty, &mut changed_nodes)?;
        }
        self.propagate(dirty, changed_nodes)
    }

    #[must_use]
    pub fn get<T>(&self, id: &SourceNodeId) -> Option<&T>
    where
        T: 'static,
    {
        self.nodes
            .get(id)
            .and_then(|node| node.value.as_ref())
            .and_then(|value| value.as_any().downcast_ref::<T>())
    }

    #[must_use]
    pub fn revision(&self, id: &SourceNodeId) -> Option<SourceNodeRevision> {
        self.nodes.get(id).map(|node| node.revision)
    }

    #[must_use]
    pub fn dependencies(&self, id: &SourceNodeId) -> Option<&[SourceNodeId]> {
        self.nodes.get(id).map(|node| match &node.kind {
            NodeKind::Input => &[] as &[SourceNodeId],
            NodeKind::Derived { deps, .. } | NodeKind::Effect { deps, .. } => deps.as_slice(),
        })
    }

    pub(super) fn read(&self) -> GraphRead<'_, E> {
        GraphRead { graph: self }
    }

    pub(super) fn ensure_new(&self, id: &SourceNodeId) -> Result<(), GraphError> {
        if self.nodes.contains_key(id) {
            return Err(GraphError::DuplicateNode(id.clone()));
        }
        Ok(())
    }
}
