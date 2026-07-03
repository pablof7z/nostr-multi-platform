use std::collections::BTreeSet;

use trellis_core::{DependencyList, DerivedNode, Graph, InputNode};

use crate::group_list::SimpleGroupRef;

const ACTIVE_ACCOUNT_NODE: &str = "nmp.nip51.simple_groups.active_account";
const RAW_LIST_NODE: &str = "nmp.nip51.simple_groups.raw_list";
const VISIBLE_GROUPS_NODE: &str = "nmp.nip51.simple_groups.visible_groups";

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct SimpleGroupListGraphStore {
    pub(super) owner_pubkey: Option<String>,
    pub(super) groups: BTreeSet<SimpleGroupRef>,
    pub(super) created_at: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum SimpleGroupListGraphEffect {
    PerspectiveChanged { groups: BTreeSet<SimpleGroupRef> },
}

pub(super) struct SimpleGroupListGraph {
    graph: Graph<()>,
    active_account: InputNode<Option<String>>,
    raw_list: InputNode<SimpleGroupListGraphStore>,
    visible_groups: DerivedNode<BTreeSet<SimpleGroupRef>>,
}

impl SimpleGroupListGraph {
    pub(super) fn new(active_account: Option<String>) -> Self {
        let mut graph = Graph::<()>::new_with_command_type();
        let mut tx = graph
            .begin_transaction()
            .expect("static simple-groups graph transaction opens");

        let active_account_node = tx
            .input::<Option<String>>(ACTIVE_ACCOUNT_NODE)
            .expect("static simple-groups active-account node registration");
        let raw_list_node = tx
            .input::<SimpleGroupListGraphStore>(RAW_LIST_NODE)
            .expect("static simple-groups raw-list node registration");
        tx.set_input(active_account_node, active_account)
            .expect("static simple-groups active-account seed");
        tx.set_input(raw_list_node, SimpleGroupListGraphStore::default())
            .expect("static simple-groups raw-list seed");

        let visible_groups_node = tx
            .derived::<BTreeSet<SimpleGroupRef>>(
                VISIBLE_GROUPS_NODE,
                DependencyList::new([active_account_node.id(), raw_list_node.id()])
                    .expect("static simple-groups visible-group dependencies"),
                move |read| {
                    let active = read.input(active_account_node)?.as_deref();
                    let store = read.input(raw_list_node)?;
                    Ok(visible_groups(active, store))
                },
            )
            .expect("static simple-groups visible-groups node registration");

        tx.commit()
            .expect("static simple-groups graph initial transaction");
        drop(tx);

        Self {
            graph,
            active_account: active_account_node,
            raw_list: raw_list_node,
            visible_groups: visible_groups_node,
        }
    }

    pub(super) fn current_visible_groups(&self) -> BTreeSet<SimpleGroupRef> {
        self.graph
            .derived_value(self.visible_groups)
            .ok()
            .flatten()
            .cloned()
            .unwrap_or_default()
    }

    pub(super) fn apply_active_source(
        &mut self,
        active_account: Option<String>,
    ) -> Vec<SimpleGroupListGraphEffect> {
        self.commit_inputs(Some(active_account), None)
    }

    pub(super) fn upsert_list(
        &mut self,
        owner: String,
        groups: BTreeSet<SimpleGroupRef>,
        created_at: u64,
    ) -> Vec<SimpleGroupListGraphEffect> {
        let mut store = self
            .graph
            .input_value(self.raw_list)
            .ok()
            .flatten()
            .cloned()
            .unwrap_or_default();

        if store.owner_pubkey.as_deref() != Some(owner.as_str()) {
            store.owner_pubkey = Some(owner);
            store.groups.clear();
            store.created_at = 0;
        }

        if created_at < store.created_at {
            return Vec::new();
        }
        if store.created_at == created_at && store.groups == groups {
            return Vec::new();
        }

        store.groups = groups;
        store.created_at = created_at;

        self.commit_inputs(None, Some(store))
    }

    fn commit_inputs(
        &mut self,
        active_account: Option<Option<String>>,
        raw_list: Option<SimpleGroupListGraphStore>,
    ) -> Vec<SimpleGroupListGraphEffect> {
        let result = {
            let mut tx = match self.graph.begin_transaction() {
                Ok(tx) => tx,
                Err(_) => return Vec::new(),
            };
            if let Some(active_account) = active_account {
                if tx.set_input(self.active_account, active_account).is_err() {
                    return Vec::new();
                }
            }
            if let Some(raw_list) = raw_list {
                if tx.set_input(self.raw_list, raw_list).is_err() {
                    return Vec::new();
                }
            }
            tx.commit()
        };

        let Ok(result) = result else {
            return Vec::new();
        };
        debug_assert!(self.graph.assert_incremental_equals_full().is_ok());

        if result
            .changed_derived_nodes
            .contains(&self.visible_groups.id())
        {
            vec![SimpleGroupListGraphEffect::PerspectiveChanged {
                groups: self.current_visible_groups(),
            }]
        } else {
            Vec::new()
        }
    }
}

fn visible_groups(
    active: Option<&str>,
    store: &SimpleGroupListGraphStore,
) -> BTreeSet<SimpleGroupRef> {
    if store.owner_pubkey.as_deref() != active {
        return BTreeSet::new();
    }
    store.groups.clone()
}

#[cfg(test)]
#[path = "group_list_graph_tests.rs"]
mod tests;
