use std::collections::{BTreeMap, BTreeSet};

use nmp_core::reactive_source_graph::{ReactiveSourceGraph, SourceInputUpdate, SourceNodeId};

const ACTIVE_ACCOUNT_NODE: &str = "nmp.nip51.people_list.active_account";
const RAW_LISTS_NODE: &str = "nmp.nip51.people_list.raw_lists";
const VISIBLE_LISTS_NODE: &str = "nmp.nip51.people_list.visible_lists";
const VISIBLE_LISTS_EFFECT_NODE: &str = "nmp.nip51.people_list.visible_lists_effect";

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
    graph: ReactiveSourceGraph<PeopleListGraphEffect>,
    active_account: SourceNodeId,
    raw_lists: SourceNodeId,
    visible_lists: SourceNodeId,
}

impl PeopleListGraph {
    pub(super) fn new(active_account: Option<String>) -> Self {
        let active_account_node = SourceNodeId::from(ACTIVE_ACCOUNT_NODE);
        let raw_lists_node = SourceNodeId::from(RAW_LISTS_NODE);
        let visible_lists_node = SourceNodeId::from(VISIBLE_LISTS_NODE);
        let effect_node = SourceNodeId::from(VISIBLE_LISTS_EFFECT_NODE);

        let mut graph = ReactiveSourceGraph::new();
        graph
            .add_input(active_account_node.clone(), active_account)
            .expect("static people-list active-account node registration");
        graph
            .add_input(raw_lists_node.clone(), PeopleListGraphStore::default())
            .expect("static people-list raw-lists node registration");

        graph
            .add_derived::<BTreeMap<String, BTreeSet<String>>, _>(
                visible_lists_node.clone(),
                [active_account_node.clone(), raw_lists_node.clone()],
                {
                    let active_account_node = active_account_node.clone();
                    let raw_lists_node = raw_lists_node.clone();
                    move |read| {
                        let active = read
                            .get::<Option<String>>(&active_account_node)
                            .and_then(Option::as_deref);
                        let store = read
                            .get::<PeopleListGraphStore>(&raw_lists_node)
                            .cloned()
                            .unwrap_or_default();
                        visible_lists(active, &store)
                    }
                },
            )
            .expect("static people-list visible-lists node registration");

        graph
            .add_effect(effect_node, [visible_lists_node.clone()], {
                let visible_lists_node = visible_lists_node.clone();
                move |read| {
                    let lists =
                        read.get::<BTreeMap<String, BTreeSet<String>>>(&visible_lists_node)?;
                    Some(PeopleListGraphEffect::PerspectiveChanged {
                        lists: lists.clone(),
                    })
                }
            })
            .expect("static people-list source-effect node registration");

        Self {
            graph,
            active_account: active_account_node,
            raw_lists: raw_lists_node,
            visible_lists: visible_lists_node,
        }
    }

    pub(super) fn current_visible_lists(&self) -> BTreeMap<String, BTreeSet<String>> {
        self.graph
            .get::<BTreeMap<String, BTreeSet<String>>>(&self.visible_lists)
            .cloned()
            .unwrap_or_default()
    }

    pub(super) fn apply_active_source(
        &mut self,
        active_account: Option<String>,
    ) -> Vec<PeopleListGraphEffect> {
        self.graph
            .set_input(self.active_account.clone(), active_account)
            .map(|turn| turn.into_effects())
            .unwrap_or_default()
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
            .get::<PeopleListGraphStore>(&self.raw_lists)
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

        self.graph
            .apply_inputs([SourceInputUpdate::new(self.raw_lists.clone(), store)])
            .map(|turn| turn.into_effects())
            .unwrap_or_default()
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
