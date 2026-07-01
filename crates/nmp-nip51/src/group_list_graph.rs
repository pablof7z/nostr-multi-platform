use std::collections::BTreeSet;

use nmp_core::reactive_source_graph::{ReactiveSourceGraph, SourceInputUpdate, SourceNodeId};

use crate::group_list::SimpleGroupRef;

const ACTIVE_ACCOUNT_NODE: &str = "nmp.nip51.simple_groups.active_account";
const RAW_LIST_NODE: &str = "nmp.nip51.simple_groups.raw_list";
const VISIBLE_GROUPS_NODE: &str = "nmp.nip51.simple_groups.visible_groups";
const VISIBLE_GROUPS_EFFECT_NODE: &str = "nmp.nip51.simple_groups.visible_groups_effect";

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
    graph: ReactiveSourceGraph<SimpleGroupListGraphEffect>,
    active_account: SourceNodeId,
    raw_list: SourceNodeId,
    visible_groups: SourceNodeId,
}

impl SimpleGroupListGraph {
    pub(super) fn new(active_account: Option<String>) -> Self {
        let active_account_node = SourceNodeId::from(ACTIVE_ACCOUNT_NODE);
        let raw_list_node = SourceNodeId::from(RAW_LIST_NODE);
        let visible_groups_node = SourceNodeId::from(VISIBLE_GROUPS_NODE);
        let effect_node = SourceNodeId::from(VISIBLE_GROUPS_EFFECT_NODE);

        let mut graph = ReactiveSourceGraph::new();
        graph
            .add_input(active_account_node.clone(), active_account)
            .expect("static simple-groups active-account node registration");
        graph
            .add_input(raw_list_node.clone(), SimpleGroupListGraphStore::default())
            .expect("static simple-groups raw-list node registration");

        graph
            .add_derived::<BTreeSet<SimpleGroupRef>, _>(
                visible_groups_node.clone(),
                [active_account_node.clone(), raw_list_node.clone()],
                {
                    let active_account_node = active_account_node.clone();
                    let raw_list_node = raw_list_node.clone();
                    move |read| {
                        let active = read
                            .get::<Option<String>>(&active_account_node)
                            .and_then(Option::as_deref);
                        let store = read
                            .get::<SimpleGroupListGraphStore>(&raw_list_node)
                            .cloned()
                            .unwrap_or_default();
                        visible_groups(active, &store)
                    }
                },
            )
            .expect("static simple-groups visible-groups node registration");

        graph
            .add_effect(effect_node, [visible_groups_node.clone()], {
                let visible_groups_node = visible_groups_node.clone();
                move |read| {
                    let groups = read.get::<BTreeSet<SimpleGroupRef>>(&visible_groups_node)?;
                    Some(SimpleGroupListGraphEffect::PerspectiveChanged {
                        groups: groups.clone(),
                    })
                }
            })
            .expect("static simple-groups source-effect node registration");

        Self {
            graph,
            active_account: active_account_node,
            raw_list: raw_list_node,
            visible_groups: visible_groups_node,
        }
    }

    pub(super) fn current_visible_groups(&self) -> BTreeSet<SimpleGroupRef> {
        self.graph
            .get::<BTreeSet<SimpleGroupRef>>(&self.visible_groups)
            .cloned()
            .unwrap_or_default()
    }

    pub(super) fn apply_active_source(
        &mut self,
        active_account: Option<String>,
    ) -> Vec<SimpleGroupListGraphEffect> {
        self.graph
            .set_input(self.active_account.clone(), active_account)
            .map(|turn| turn.into_effects())
            .unwrap_or_default()
    }

    pub(super) fn upsert_list(
        &mut self,
        owner: String,
        groups: BTreeSet<SimpleGroupRef>,
        created_at: u64,
    ) -> Vec<SimpleGroupListGraphEffect> {
        let mut store = self
            .graph
            .get::<SimpleGroupListGraphStore>(&self.raw_list)
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

        self.graph
            .apply_inputs([SourceInputUpdate::new(self.raw_list.clone(), store)])
            .map(|turn| turn.into_effects())
            .unwrap_or_default()
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
