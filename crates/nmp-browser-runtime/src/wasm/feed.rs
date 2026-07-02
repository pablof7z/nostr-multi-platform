//! Feed-session Worker controls.

use super::core::NmpRuntimeCore;
use super::dispatch_support::not_started_error;
use super::protocol::{FeedHandleRequest, FeedOpenJson, WorkerEvent};

impl NmpRuntimeCore {
    pub(super) fn handle_feed_open_json(&mut self, req: FeedOpenJson) -> Vec<WorkerEvent> {
        let Some(handle) = self.handle.as_mut() else {
            return not_started_error(Some(req.correlation_id));
        };

        let params = match serde_json::from_str::<nmp_feed::FeedParams>(&req.params_json) {
            Ok(params) => params,
            Err(err) => {
                return vec![WorkerEvent::Error {
                    code: "feed_params_rejected".to_string(),
                    message: format!("FeedParams deserialize failed: {err}"),
                    correlation_id: Some(req.correlation_id),
                }];
            }
        };

        match handle.feeds().open(params) {
            Some(feed_handle) => vec![WorkerEvent::FeedOpened {
                handle: feed_handle,
                correlation_id: req.correlation_id,
            }],
            None => vec![WorkerEvent::CapabilityFailure {
                capability: "nmp.feed.open".to_string(),
                correlation_id: req.correlation_id,
                reason: "feed_open_failed".to_string(),
            }],
        }
    }

    pub(super) fn handle_feed_load_older(&mut self, req: FeedHandleRequest) -> Vec<WorkerEvent> {
        let Some(handle) = self.handle.as_mut() else {
            return not_started_error(Some(req.correlation_id));
        };

        let _ = handle.feeds().load_older(&req.handle);
        vec![WorkerEvent::ActionAccepted {
            action_type: "nmp.feed.load_older".to_string(),
            correlation_id: req.correlation_id,
        }]
    }

    pub(super) fn handle_feed_close(&mut self, req: FeedHandleRequest) -> Vec<WorkerEvent> {
        let Some(handle) = self.handle.as_mut() else {
            return not_started_error(Some(req.correlation_id));
        };

        let _ = handle.feeds().close(&req.handle);
        vec![WorkerEvent::ActionAccepted {
            action_type: "nmp.feed.close".to_string(),
            correlation_id: req.correlation_id,
        }]
    }
}
