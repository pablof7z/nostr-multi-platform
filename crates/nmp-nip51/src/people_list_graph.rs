use std::collections::{BTreeMap, BTreeSet};

use trellis_core::{DependencyList, DerivedNode, Graph, InputNode};

const ACTIVE_ACCOUNT_NODE: &str = "nmp.nip51.people_list.active_account";
const RAW_LISTS_NODE: &str = "nmp.nip51.people_list.raw_lists";
const VISIBLE_LISTS_NODE: &str = "nmp.nip51.people_list.visible_lists";

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct PeopleListGraphEntry {
    pub(super) members: BTreeSet<String>,
    pub(super) created_at: u64,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(super) struct PeopleListGraphStore {
    pub(super) owner_pubkey: Option<String>,
    pub(super) lists: BTreeMap<String, PeopleListGraphEntry>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum PeopleListGraphEffect {
    PerspectiveChanged {
        lists: BTreeMap<String, BTreeSet<String>>,
    },
}

pub(super) struct PeopleListGraph {
    graph: Graph<()>,
    active_account: InputNode<Option<String>>,
    raw_lists: InputNode<PeopleListGraphStore>,
    visible_lists: DerivedNode<BTreeMap<String, BTreeSet<String>>>,
}

impl PeopleListGraph {
    pub(super) fn new(active_account: Option<String>) -> Self {
        let mut graph = Graph::<()>::new_with_command_type();
        let mut tx = graph
            .begin_transaction()
            .expect("static people-list graph transaction opens");

        let active_account_node = tx
            .input::<Option<String>>(ACTIVE_ACCOUNT_NODE)
            .expect("static people-list active-account node registration");
        let raw_lists_node = tx
            .input::<PeopleListGraphStore>(RAW_LISTS_NODE)
            .expect("static people-list raw-lists node registration");
        tx.set_input(active_account_node, active_account)
            .expect("static people-list active-account seed");
        tx.set_input(raw_lists_node, PeopleListGraphStore::default())
            .expect("static people-list raw-lists seed");

        let visible_lists_node = tx
            .derived::<BTreeMap<String, BTreeSet<String>>>(
                VISIBLE_LISTS_NODE,
                DependencyList::new([active_account_node.id(), raw_lists_node.id()])
                    .expect("static people-list visible-list dependencies"),
                move |read| {
                    let active = read.input(active_account_node)?.as_deref();
                    let store = read.input(raw_lists_node)?;
                    Ok(visible_lists(active, store))
                },
            )
            .expect("static people-list visible-lists node registration");

        tx.commit()
            .expect("static people-list graph initial transaction");
        drop(tx);

        Self {
            graph,
            active_account: active_account_node,
            raw_lists: raw_lists_node,
            visible_lists: visible_lists_node,
        }
    }

    pub(super) fn current_visible_lists(&self) -> BTreeMap<String, BTreeSet<String>> {
        self.graph
            .derived_value(self.visible_lists)
            .ok()
            .flatten()
            .cloned()
            .unwrap_or_default()
    }

    pub(super) fn apply_active_source(
        &mut self,
        active_account: Option<String>,
    ) -> Vec<PeopleListGraphEffect> {
        self.commit_inputs(Some(active_account), None)
    }

    pub(super) fn upsert_list(
        &mut self,
        owner: String,
        list_id: String,
        members: BTreeSet<String>,
        created_at: u64,
    ) -> Vec<PeopleListGraphEffect> {
        let mut store = self
            .graph
            .input_value(self.raw_lists)
            .ok()
            .flatten()
            .cloned()
            .unwrap_or_default();

        if store.owner_pubkey.as_deref() != Some(owner.as_str()) {
            store.owner_pubkey = Some(owner);
            store.lists.clear();
        }

        match store.lists.get(&list_id) {
            Some(existing) if created_at < existing.created_at => return Vec::new(),
            Some(existing) if existing.members == members && existing.created_at == created_at => {
                return Vec::new();
            }
            _ => {
                store.lists.insert(
                    list_id,
                    PeopleListGraphEntry {
                        members,
                        created_at,
                    },
                );
            }
        }

        self.commit_inputs(None, Some(store))
    }

    fn commit_inputs(
        &mut self,
        active_account: Option<Option<String>>,
        raw_lists: Option<PeopleListGraphStore>,
    ) -> Vec<PeopleListGraphEffect> {
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
            if let Some(raw_lists) = raw_lists {
                if tx.set_input(self.raw_lists, raw_lists).is_err() {
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
            .contains(&self.visible_lists.id())
        {
            vec![PeopleListGraphEffect::PerspectiveChanged {
                lists: self.current_visible_lists(),
            }]
        } else {
            Vec::new()
        }
    }
}

fn visible_lists(
    active: Option<&str>,
    store: &PeopleListGraphStore,
) -> BTreeMap<String, BTreeSet<String>> {
    if store.owner_pubkey.as_deref() != active {
        return BTreeMap::new();
    }
    store
        .lists
        .iter()
        .map(|(list_id, entry)| (list_id.clone(), entry.members.clone()))
        .collect()
}

#[cfg(test)]
#[path = "people_list_graph_tests.rs"]
mod tests;
