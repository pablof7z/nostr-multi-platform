use std::collections::BTreeSet;
use std::sync::Mutex;

use nmp_core::substrate::KernelEvent;
use nmp_core::ObservedProjectionSink;
use nmp_wot::score::WotGraph;

/// WoT ranked-candidate cap (the #1698 query takes a limit; 0 = unlimited).
const WOT_CANDIDATE_LIMIT: usize = 500;

/// A minimal kind:3-ingesting WoT graph for ONE feed session, reusing
/// [`nmp_wot::score::WotGraph`]'s ranked second-degree query (#1698). It does
/// not duplicate the ranking logic: it owns a `WotGraph`, feeds it kind:3
/// edges, and reads `ranked_second_degree_candidates`.
pub(super) struct SessionWotGraph {
    seed: String,
    contact_kind: u32,
    graph: Mutex<WotGraph>,
    /// The seed's DIRECT follows (from the seed's own kind:3), tracked so the
    /// session can acquire their contact lists for second-degree ranking.
    direct: Mutex<BTreeSet<String>>,
    /// Cached ranked candidate set: recomputed once per graph change, not once
    /// per admission test.
    ranked: Mutex<BTreeSet<String>>,
    on_change: Mutex<Vec<Box<dyn Fn() + Send + Sync>>>,
}

impl SessionWotGraph {
    pub(super) fn new(seed: String, contact_kind: u32) -> Self {
        Self {
            seed,
            contact_kind,
            graph: Mutex::new(WotGraph::default()),
            direct: Mutex::new(BTreeSet::new()),
            ranked: Mutex::new(BTreeSet::new()),
            on_change: Mutex::new(Vec::new()),
        }
    }

    /// The current ranked second-degree candidate set (cached).
    pub(super) fn ranked_candidates(&self) -> BTreeSet<String> {
        self.ranked.lock().map(|r| r.clone()).unwrap_or_default()
    }

    /// The seed's direct follows (their kind:3 feeds the ranking).
    pub(super) fn direct_follows(&self) -> BTreeSet<String> {
        self.direct.lock().map(|d| d.clone()).unwrap_or_default()
    }

    pub(super) fn admits(&self, pk: &str) -> bool {
        self.ranked.lock().map(|r| r.contains(pk)).unwrap_or(false)
    }

    pub(super) fn on_change(&self, cb: Box<dyn Fn() + Send + Sync>) {
        if let Ok(mut cbs) = self.on_change.lock() {
            cbs.push(cb);
        }
    }

    fn fire(&self) {
        if let Ok(cbs) = self.on_change.lock() {
            for cb in cbs.iter() {
                cb();
            }
        }
    }
}

impl ObservedProjectionSink for SessionWotGraph {
    fn on_kernel_event(&self, event: &KernelEvent) {
        if event.kind != self.contact_kind {
            return;
        }
        // Track the seed's direct follows from the seed's own kind:3.
        if event.author == self.seed {
            let follows: BTreeSet<String> = event
                .tags
                .iter()
                .filter_map(|tag| {
                    if tag.first().is_some_and(|t| t == "p") {
                        tag.get(1).cloned()
                    } else {
                        None
                    }
                })
                .collect();
            if let Ok(mut direct) = self.direct.lock() {
                *direct = follows;
            }
        }

        let ranked: BTreeSet<String> = {
            let Ok(mut graph) = self.graph.lock() else {
                return;
            };
            graph.ingest_event(&event.author, event.kind, &event.tags);
            graph
                .ranked_second_degree_candidates(&self.seed, WOT_CANDIDATE_LIMIT)
                .into_iter()
                .map(|(pk, _)| pk)
                .collect()
        };
        if let Ok(mut cache) = self.ranked.lock() {
            *cache = ranked;
        }
        self.fire();
    }
}
