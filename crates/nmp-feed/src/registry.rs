use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

pub trait FeedController: Send + Sync {
    fn snapshot_json(&self) -> serde_json::Value;
    fn load_older(&self) -> bool;
}

#[derive(Default)]
pub struct FeedRegistry {
    feeds: Mutex<BTreeMap<String, Arc<dyn FeedController>>>,
}

impl FeedRegistry {
    pub fn register(&self, key: impl Into<String>, controller: Arc<dyn FeedController>) {
        if let Ok(mut feeds) = self.feeds.lock() {
            feeds.insert(key.into(), controller);
        }
    }

    #[must_use]
    pub fn load_older(&self, key: &str) -> bool {
        let controller = self
            .feeds
            .lock()
            .ok()
            .and_then(|feeds| feeds.get(key).cloned());
        controller.is_some_and(|controller| controller.load_older())
    }
}

pub type FeedRegistrySlot = Arc<FeedRegistry>;

#[must_use]
pub fn new_feed_registry_slot() -> FeedRegistrySlot {
    Arc::new(FeedRegistry::default())
}
