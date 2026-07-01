use std::collections::BTreeSet;

use nmp_core::reactive_source_graph::{ReactiveSourceGraph, SourceInputUpdate, SourceNodeId};

const ACTIVE_ACCOUNT_NODE: &str = "nmp.nip02.active_follow_set.active_account";
const CONTACT_FOLLOWS_NODE: &str = "nmp.nip02.active_follow_set.contact_follows";
const ACTIVE_FOLLOWS_NODE: &str = "nmp.nip02.active_follow_set.active_follows";
const PERSPECTIVE_NODE: &str = "nmp.nip02.active_follow_set.perspective";
const PERSPECTIVE_EFFECT_NODE: &str = "nmp.nip02.active_follow_set.perspective_effect";

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum ActiveFollowGraphEffect {
    PerspectiveChanged { follows: BTreeSet<String> },
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ActiveFollowPerspective {
    active_account: Option<String>,
    follows: BTreeSet<String>,
}

pub(super) struct ActiveFollowGraph {
    graph: ReactiveSourceGraph<ActiveFollowGraphEffect>,
    active_account: SourceNodeId,
    contact_follows: SourceNodeId,
    active_follows: SourceNodeId,
}

impl ActiveFollowGraph {
    pub(super) fn new(active_account: Option<String>, contact_follows: BTreeSet<String>) -> Self {
        let active_account_node = SourceNodeId::from(ACTIVE_ACCOUNT_NODE);
        let contact_follows_node = SourceNodeId::from(CONTACT_FOLLOWS_NODE);
        let active_follows_node = SourceNodeId::from(ACTIVE_FOLLOWS_NODE);
        let perspective_node = SourceNodeId::from(PERSPECTIVE_NODE);
        let effect_node = SourceNodeId::from(PERSPECTIVE_EFFECT_NODE);

        let mut graph = ReactiveSourceGraph::new();
        graph
            .add_input(active_account_node.clone(), active_account)
            .expect("static active-account node registration");
        graph
            .add_input(contact_follows_node.clone(), contact_follows)
            .expect("static contact-follows node registration");

        graph
            .add_derived::<BTreeSet<String>, _>(
                active_follows_node.clone(),
                [active_account_node.clone(), contact_follows_node.clone()],
                {
                    let active_account_node = active_account_node.clone();
                    let contact_follows_node = contact_follows_node.clone();
                    move |read| {
                        let Some(Some(active)) = read.get::<Option<String>>(&active_account_node)
                        else {
                            return BTreeSet::new();
                        };
                        let mut follows = read
                            .get::<BTreeSet<String>>(&contact_follows_node)
                            .cloned()
                            .unwrap_or_default();
                        follows.insert(active.clone());
                        follows
                    }
                },
            )
            .expect("static active-follows node registration");

        graph
            .add_derived::<ActiveFollowPerspective, _>(
                perspective_node.clone(),
                [active_account_node.clone(), active_follows_node.clone()],
                {
                    let active_account_node = active_account_node.clone();
                    let active_follows_node = active_follows_node.clone();
                    move |read| ActiveFollowPerspective {
                        active_account: read
                            .get::<Option<String>>(&active_account_node)
                            .cloned()
                            .unwrap_or_default(),
                        follows: read
                            .get::<BTreeSet<String>>(&active_follows_node)
                            .cloned()
                            .unwrap_or_default(),
                    }
                },
            )
            .expect("static active-follow perspective node registration");

        graph
            .add_effect(effect_node, [perspective_node.clone()], {
                let perspective_node = perspective_node.clone();
                move |read| {
                    let perspective = read.get::<ActiveFollowPerspective>(&perspective_node)?;
                    Some(ActiveFollowGraphEffect::PerspectiveChanged {
                        follows: perspective.follows.clone(),
                    })
                }
            })
            .expect("static active-follow perspective effect registration");

        Self {
            graph,
            active_account: active_account_node,
            contact_follows: contact_follows_node,
            active_follows: active_follows_node,
        }
    }

    pub(super) fn current_follows(&self) -> BTreeSet<String> {
        self.graph
            .get::<BTreeSet<String>>(&self.active_follows)
            .cloned()
            .unwrap_or_default()
    }

    pub(super) fn apply_active_source(
        &mut self,
        active_account: Option<String>,
        contact_follows: BTreeSet<String>,
    ) -> Vec<ActiveFollowGraphEffect> {
        self.graph
            .apply_inputs([
                SourceInputUpdate::new(self.active_account.clone(), active_account),
                SourceInputUpdate::new(self.contact_follows.clone(), contact_follows),
            ])
            .map(|turn| turn.into_effects())
            .unwrap_or_default()
    }
}
