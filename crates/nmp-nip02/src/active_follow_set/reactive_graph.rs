use std::collections::BTreeSet;

use trellis_core::{DependencyList, DerivedNode, Graph, InputNode};

const ACTIVE_ACCOUNT_NODE: &str = "nmp.nip02.active_follow_set.active_account";
const CONTACT_FOLLOWS_NODE: &str = "nmp.nip02.active_follow_set.contact_follows";
const ACTIVE_FOLLOWS_NODE: &str = "nmp.nip02.active_follow_set.active_follows";
const PERSPECTIVE_NODE: &str = "nmp.nip02.active_follow_set.perspective";

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
    graph: Graph<()>,
    active_account: InputNode<Option<String>>,
    contact_follows: InputNode<BTreeSet<String>>,
    active_follows: DerivedNode<BTreeSet<String>>,
    perspective: DerivedNode<ActiveFollowPerspective>,
}

impl ActiveFollowGraph {
    pub(super) fn new(active_account: Option<String>, contact_follows: BTreeSet<String>) -> Self {
        let mut graph = Graph::<()>::new_with_command_type();
        let mut tx = graph
            .begin_transaction()
            .expect("static active-follow graph transaction opens");

        let active_account_node = tx
            .input::<Option<String>>(ACTIVE_ACCOUNT_NODE)
            .expect("static active-account node registration");
        let contact_follows_node = tx
            .input::<BTreeSet<String>>(CONTACT_FOLLOWS_NODE)
            .expect("static contact-follows node registration");
        tx.set_input(active_account_node, active_account)
            .expect("static active-account seed");
        tx.set_input(contact_follows_node, contact_follows)
            .expect("static contact-follows seed");

        let active_follows_node = tx
            .derived::<BTreeSet<String>>(
                ACTIVE_FOLLOWS_NODE,
                DependencyList::new([active_account_node.id(), contact_follows_node.id()])
                    .expect("static active-follows dependencies"),
                move |read| {
                    let Some(active) = read.input(active_account_node)?.as_ref() else {
                        return Ok(BTreeSet::new());
                    };
                    let mut follows = read.input(contact_follows_node)?.clone();
                    follows.insert(active.clone());
                    Ok(follows)
                },
            )
            .expect("static active-follows node registration");

        let perspective_node = tx
            .derived::<ActiveFollowPerspective>(
                PERSPECTIVE_NODE,
                DependencyList::new([active_account_node.id(), active_follows_node.id()])
                    .expect("static perspective dependencies"),
                move |read| {
                    Ok(ActiveFollowPerspective {
                        active_account: read.input(active_account_node)?.clone(),
                        follows: read.derived(active_follows_node)?.clone(),
                    })
                },
            )
            .expect("static active-follow perspective node registration");

        tx.commit()
            .expect("static active-follow graph initial transaction");
        drop(tx);

        Self {
            graph,
            active_account: active_account_node,
            contact_follows: contact_follows_node,
            active_follows: active_follows_node,
            perspective: perspective_node,
        }
    }

    pub(super) fn current_follows(&self) -> BTreeSet<String> {
        self.graph
            .derived_value(self.active_follows)
            .ok()
            .flatten()
            .cloned()
            .unwrap_or_default()
    }

    pub(super) fn apply_active_source(
        &mut self,
        active_account: Option<String>,
        contact_follows: BTreeSet<String>,
    ) -> Vec<ActiveFollowGraphEffect> {
        let result = {
            let mut tx = match self.graph.begin_transaction() {
                Ok(tx) => tx,
                Err(_) => return Vec::new(),
            };
            if tx.set_input(self.active_account, active_account).is_err()
                || tx.set_input(self.contact_follows, contact_follows).is_err()
            {
                return Vec::new();
            }
            tx.commit()
        };

        let Ok(result) = result else {
            return Vec::new();
        };
        debug_assert!(self.graph.assert_incremental_equals_full().is_ok());

        if result
            .changed_derived_nodes
            .contains(&self.perspective.id())
        {
            vec![ActiveFollowGraphEffect::PerspectiveChanged {
                follows: self.current_follows(),
            }]
        } else {
            Vec::new()
        }
    }
}
