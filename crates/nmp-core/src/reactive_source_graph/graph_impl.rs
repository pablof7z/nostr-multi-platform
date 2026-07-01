use std::collections::BTreeSet;

use super::graph::{
    GraphError, GraphRead, GraphTurn, NodeChange, NodeKind, ReactiveSourceGraph, SourceInputUpdate,
    SourceNode,
};
use super::id::SourceNodeId;
use super::value::SourceValue;

enum Recompute<E> {
    Derived(Box<dyn SourceValue>),
    Effect(Option<E>),
    Input,
}

impl<E> ReactiveSourceGraph<E> {
    pub(super) fn validate_deps(
        &self,
        id: &SourceNodeId,
        deps: &[SourceNodeId],
    ) -> Result<(), GraphError> {
        self.ensure_new(id)?;
        for dep in deps {
            if dep == id || !self.nodes.contains_key(dep) {
                return Err(GraphError::MissingDependency {
                    node: id.clone(),
                    dependency: dep.clone(),
                });
            }
        }
        Ok(())
    }

    pub(super) fn insert_dependent_node(
        &mut self,
        id: SourceNodeId,
        deps: Vec<SourceNodeId>,
        node: SourceNode<E>,
    ) {
        for dep in deps {
            self.dependents.entry(dep).or_default().insert(id.clone());
        }
        self.nodes.insert(id, node);
    }

    pub(super) fn apply_one_input(
        &mut self,
        update: SourceInputUpdate,
        dirty: &mut BTreeSet<SourceNodeId>,
        changed_nodes: &mut Vec<NodeChange>,
    ) -> Result<bool, GraphError> {
        let id = update.id;
        let node = self
            .nodes
            .get_mut(&id)
            .ok_or_else(|| GraphError::UnknownNode(id.clone()))?;
        if !matches!(node.kind, NodeKind::Input) {
            return Err(GraphError::NotInput(id));
        }
        let current = node
            .value
            .as_ref()
            .ok_or_else(|| GraphError::MissingValue(id.clone()))?;
        if current.as_any().type_id() != update.value.as_any().type_id() {
            return Err(GraphError::ValueTypeMismatch {
                node: id,
                expected: current.type_name(),
                actual: update.value.type_name(),
            });
        }
        if current.equals(update.value.as_ref()) {
            return Ok(false);
        }
        node.value = Some(update.value);
        node.revision.bump();
        dirty.insert(id.clone());
        changed_nodes.push(NodeChange {
            id,
            revision: node.revision,
        });
        Ok(true)
    }

    pub(super) fn propagate(
        &mut self,
        mut dirty: BTreeSet<SourceNodeId>,
        mut changed_nodes: Vec<NodeChange>,
    ) -> Result<GraphTurn<E>, GraphError> {
        let mut effects = Vec::new();
        while !dirty.is_empty() {
            let candidates = self.candidate_dependents(&dirty);
            dirty.clear();
            for id in candidates {
                match self.recompute(&id)? {
                    Recompute::Input => {}
                    Recompute::Derived(next) => {
                        if self.replace_if_changed(&id, next, &mut changed_nodes)? {
                            dirty.insert(id);
                        }
                    }
                    Recompute::Effect(effect) => {
                        if let Some(effect) = effect {
                            self.bump_effect(&id, &mut changed_nodes)?;
                            effects.push(effect);
                        }
                    }
                }
            }
        }
        Ok(GraphTurn {
            changed_nodes,
            effects,
        })
    }

    fn candidate_dependents(&self, dirty: &BTreeSet<SourceNodeId>) -> BTreeSet<SourceNodeId> {
        let mut candidates = BTreeSet::new();
        for id in dirty {
            if let Some(dependents) = self.dependents.get(id) {
                candidates.extend(dependents.iter().cloned());
            }
        }
        candidates
    }

    fn recompute(&self, id: &SourceNodeId) -> Result<Recompute<E>, GraphError> {
        let node = self
            .nodes
            .get(id)
            .ok_or_else(|| GraphError::UnknownNode(id.clone()))?;
        let read = GraphRead { graph: self };
        Ok(match &node.kind {
            NodeKind::Input => Recompute::Input,
            NodeKind::Derived { reducer, .. } => Recompute::Derived(reducer(&read)),
            NodeKind::Effect { effect, .. } => Recompute::Effect(effect(&read)),
        })
    }

    fn replace_if_changed(
        &mut self,
        id: &SourceNodeId,
        next: Box<dyn SourceValue>,
        changed_nodes: &mut Vec<NodeChange>,
    ) -> Result<bool, GraphError> {
        let node = self
            .nodes
            .get_mut(id)
            .ok_or_else(|| GraphError::UnknownNode(id.clone()))?;
        let current = node
            .value
            .as_ref()
            .ok_or_else(|| GraphError::MissingValue(id.clone()))?;
        if current.as_any().type_id() != next.as_any().type_id() {
            return Err(GraphError::ValueTypeMismatch {
                node: id.clone(),
                expected: current.type_name(),
                actual: next.type_name(),
            });
        }
        if current.equals(next.as_ref()) {
            return Ok(false);
        }
        node.value = Some(next);
        node.revision.bump();
        changed_nodes.push(NodeChange {
            id: id.clone(),
            revision: node.revision,
        });
        Ok(true)
    }

    fn bump_effect(
        &mut self,
        id: &SourceNodeId,
        changed_nodes: &mut Vec<NodeChange>,
    ) -> Result<(), GraphError> {
        let node = self
            .nodes
            .get_mut(id)
            .ok_or_else(|| GraphError::UnknownNode(id.clone()))?;
        node.revision.bump();
        changed_nodes.push(NodeChange {
            id: id.clone(),
            revision: node.revision,
        });
        Ok(())
    }
}
