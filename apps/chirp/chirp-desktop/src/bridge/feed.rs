use std::collections::{BTreeSet, HashMap};
use std::ffi::{CStr, CString};

use nmp_app_chirp::{nmp_app_close_feed, nmp_app_open_feed};
use nmp_feed::{
    FeedAdmission, FeedParams, FeedRanking, FeedRender, FeedScope, FeedWindow, ProjectionKey,
    DEFAULT_FEED_WINDOW_LIMIT,
};
use nmp_ffi::{nmp_free_string, NmpApp};
use serde_json::Value;

use super::{AppRuntime, OPEN_PROFILE_CONSUMER};

#[derive(Default)]
pub(super) struct FeedHandles {
    pub(super) home: Option<String>,
    pub(super) authors: HashMap<String, String>,
    pub(super) threads: HashMap<String, String>,
}

pub(super) fn open_home_feed(app: *mut NmpApp) -> Result<String, String> {
    open_feed(app, &home_feed_params())
}

impl AppRuntime {
    pub fn open_timeline(&self) {
        if self.feed_handles.borrow().home.is_some() {
            return;
        }
        if let Ok(handle) = open_home_feed(self.app) {
            self.feed_handles.borrow_mut().home = Some(handle);
        }
    }

    pub fn open_thread(&self, event_id: &str) {
        let mut handles = self.feed_handles.borrow_mut();
        if handles.threads.contains_key(event_id) {
            return;
        }
        if let Ok(handle) = open_feed(self.app, &thread_feed_params(event_id)) {
            handles.threads.insert(event_id.to_string(), handle);
        }
    }

    pub fn close_thread(&self, event_id: &str) {
        if let Some(handle) = self.feed_handles.borrow_mut().threads.remove(event_id) {
            close_feed(self.app, &handle);
        }
    }

    pub fn open_author(&self, pubkey: &str) {
        {
            let mut handles = self.feed_handles.borrow_mut();
            if !handles.authors.contains_key(pubkey) {
                if let Ok(handle) = open_feed(self.app, &author_feed_params(pubkey)) {
                    handles.authors.insert(pubkey.to_string(), handle);
                }
            }
        }
        self.resolve_profile_card_live(pubkey);
    }

    pub fn close_author(&self, pubkey: &str) {
        if let Some(handle) = self.feed_handles.borrow_mut().authors.remove(pubkey) {
            close_feed(self.app, &handle);
        }
        self.release_ref(OPEN_PROFILE_CONSUMER, pubkey);
    }

    pub(super) fn close_all_feeds(&self) {
        let mut handles = self.feed_handles.borrow_mut();
        if let Some(handle) = handles.home.take() {
            close_feed(self.app, &handle);
        }
        for (_, handle) in handles.authors.drain() {
            close_feed(self.app, &handle);
        }
        for (_, handle) in handles.threads.drain() {
            close_feed(self.app, &handle);
        }
    }
}

fn home_feed_params() -> FeedParams {
    FeedParams {
        primary_kinds: vec![1],
        render: FeedRender::OpCentric,
        acquisition: FeedScope::ActiveUserFollows,
        admission: FeedAdmission::All,
        ranking: FeedRanking::ChronologicalDesc,
        window: default_window(),
        projection: ProjectionKey("nmp.feed.home".to_string()),
    }
}

fn author_feed_params(pubkey: &str) -> FeedParams {
    FeedParams {
        primary_kinds: vec![1],
        render: FeedRender::Flat,
        acquisition: FeedScope::Authors {
            authors: BTreeSet::from([pubkey.to_string()]),
        },
        admission: FeedAdmission::All,
        ranking: FeedRanking::ChronologicalDesc,
        window: default_window(),
        projection: ProjectionKey(format!("nmp.feed.author.{pubkey}")),
    }
}

fn thread_feed_params(event_id: &str) -> FeedParams {
    FeedParams {
        primary_kinds: vec![1],
        render: FeedRender::Flat,
        acquisition: FeedScope::Referrer {
            event_id: event_id.to_string(),
        },
        admission: FeedAdmission::All,
        ranking: FeedRanking::ChronologicalDesc,
        window: default_window(),
        projection: ProjectionKey(format!("nmp.feed.thread.{event_id}")),
    }
}

fn default_window() -> FeedWindow {
    FeedWindow {
        initial_limit: DEFAULT_FEED_WINDOW_LIMIT,
    }
}

fn open_feed(app: *mut NmpApp, params: &FeedParams) -> Result<String, String> {
    if app.is_null() {
        return Err("runtime app is not available".to_string());
    }
    let params_json = serde_json::to_string(params)
        .map_err(|_| "feed params could not be serialized".to_string())?;
    let params_c =
        CString::new(params_json).map_err(|_| "feed params contain NUL byte".to_string())?;
    let raw = nmp_app_open_feed(app, params_c.as_ptr());
    if raw.is_null() {
        return Err("nmp_app_open_feed returned null".to_string());
    }
    let handle_json = unsafe { CStr::from_ptr(raw) }
        .to_string_lossy()
        .into_owned();
    nmp_free_string(raw);
    if let Ok(value) = serde_json::from_str::<Value>(&handle_json) {
        if let Some(error) = value.get("error").and_then(Value::as_str) {
            return Err(format!("nmp_app_open_feed failed: {error}"));
        }
    }
    Ok(handle_json)
}

fn close_feed(app: *mut NmpApp, handle_json: &str) {
    if app.is_null() {
        return;
    }
    if let Ok(handle) = CString::new(handle_json) {
        nmp_app_close_feed(app, handle.as_ptr());
    }
}
