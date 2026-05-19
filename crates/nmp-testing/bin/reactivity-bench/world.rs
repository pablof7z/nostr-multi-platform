use crate::config::{StreamKind, ViewMix, WORKING_SET_TARGET_VIEWS};
use crate::domain::{DeltaBuffer, Dependencies, Event, PendingDeltaKind, ReverseIndex, SmallTags};
use crate::rng::Lcg;
use std::mem::size_of;

pub(crate) struct BenchWorld {
    pub(crate) author_count: u32,
    pub(crate) cached_events: usize,
    pub(crate) hot_event_limit: usize,
    pub(crate) hot_events: Vec<Event>,
    pub(crate) hot_cursor: usize,
    pub(crate) views: Vec<ViewState>,
    pub(crate) lookup_scratch: Vec<usize>,
    pub(crate) index: ReverseIndex,
    pub(crate) next_event_id: u64,
    pub(crate) profile_fanout_hits: u64,
    pub(crate) thread_root_id: u32,
    pub(crate) thread_reply_count: u64,
    pub(crate) thread_reaction_count: u64,
    pub(crate) hot_event_content_bytes: usize,
    pub(crate) event_id_checksum: u64,
}

impl BenchWorld {
    pub(crate) fn new(author_count: u32, cached_events: usize, hot_event_limit: usize) -> Self {
        Self {
            author_count,
            cached_events,
            hot_event_limit,
            hot_events: Vec::with_capacity(hot_event_limit),
            hot_cursor: 0,
            views: Vec::new(),
            lookup_scratch: Vec::new(),
            index: ReverseIndex::new(),
            next_event_id: 1,
            profile_fanout_hits: 0,
            thread_root_id: 1,
            thread_reply_count: 0,
            thread_reaction_count: 0,
            hot_event_content_bytes: 0,
            event_id_checksum: 0,
        }
    }

    pub(crate) fn prepopulate(&mut self, rng: &mut Lcg) {
        let hot_count = self.cached_events.min(self.hot_event_limit);
        self.hot_events.reserve(hot_count);
        for index in 0..hot_count {
            let event = self.synthetic_mixed_event(index, rng);
            self.hot_event_content_bytes = self
                .hot_event_content_bytes
                .saturating_add(event.content_len);
            self.event_id_checksum ^= event.id;
            self.hot_events.push(event);
        }
        self.next_event_id = self.cached_events as u64 + 1;
        self.hot_cursor = 0;
    }

    pub(crate) fn open_views(&mut self, mix: ViewMix) {
        self.views.clear();
        self.index.clear();
        match mix {
            ViewMix::QuietIdle => {
                self.views.reserve(10);
                for i in 0..5 {
                    self.add_timeline_view(author_window(i * 20, 20, self.author_count), false);
                    self.add_profile_view(i as u32);
                }
            }
            ViewMix::FollowingTimeline => {
                self.add_timeline_view(author_window(0, 1_000, self.author_count), false);
            }
            ViewMix::HashtagFirehose => {
                self.add_timeline_view(Vec::new(), true);
            }
            ViewMix::ProfileFanout => {
                self.views.reserve(50);
                for i in 0..50 {
                    let mut authors = author_window(i * 10, 1_000, self.author_count);
                    if !authors.contains(&42) {
                        authors[0] = 42;
                    }
                    self.add_timeline_view(authors, false);
                }
            }
            ViewMix::ThreadBlowup => {
                self.add_thread_view(self.thread_root_id);
                self.add_reactions_view(self.thread_root_id);
            }
            ViewMix::AccountSwitch => {
                self.open_account_views(0);
            }
            ViewMix::WorkingSet100Views => {
                self.views.reserve(WORKING_SET_TARGET_VIEWS);
                for i in 0..WORKING_SET_TARGET_VIEWS {
                    self.add_timeline_view(author_window(i * 100, 100, self.author_count), false);
                }
            }
        }
        self.lookup_scratch.reserve(self.views.len());
        self.index.reserve_for_views(self.views.len());
    }

    pub(crate) fn add_profile_view(&mut self, author: u32) {
        let deps = Dependencies {
            kind_author_pairs: vec![(0, author), (10002, author)],
            ..Dependencies::empty()
        };
        self.add_view(ViewState::Profile { author, updates: 0 }, deps);
    }

    pub(crate) fn add_timeline_view(&mut self, authors: Vec<u32>, hashtag: bool) {
        let deps = if hashtag {
            Dependencies {
                catch_all: true,
                ..Dependencies::empty()
            }
        } else {
            let mut kind_author_pairs = Vec::with_capacity(authors.len());
            for author in &authors {
                kind_author_pairs.push((0, *author));
            }
            Dependencies {
                kinds: vec![1, 6, 7],
                authors: authors.clone(),
                kind_author_pairs,
                ..Dependencies::empty()
            }
        };
        self.add_view(
            ViewState::Timeline {
                authors,
                hashtag,
                items: 0,
                updates: 0,
            },
            deps,
        );
    }

    pub(crate) fn add_thread_view(&mut self, root_id: u32) {
        let deps = Dependencies {
            kinds: vec![1, 7],
            e_tag_refs: vec![root_id],
            ..Dependencies::empty()
        };
        self.add_view(
            ViewState::Thread {
                root_id,
                replies: 0,
                reactions: 0,
            },
            deps,
        );
    }

    pub(crate) fn add_reactions_view(&mut self, target_id: u32) {
        let deps = Dependencies {
            kinds: vec![7],
            e_tag_refs: vec![target_id],
            ..Dependencies::empty()
        };
        self.add_view(
            ViewState::Reactions {
                target_id,
                total: 0,
            },
            deps,
        );
    }

    pub(crate) fn add_view(&mut self, view: ViewState, deps: Dependencies) {
        let view_id = self.views.len();
        self.views.push(view);
        self.index.register(view_id, &deps);
    }

    pub(crate) fn open_account_views(&mut self, account: u32) {
        self.views.clear();
        self.index.clear();
        let start = account * 100;
        self.add_timeline_view(author_window(start as usize, 100, self.author_count), false);
        self.add_profile_view(start % self.author_count);
        self.add_profile_view((start + 1) % self.author_count);
        self.add_thread_view(self.thread_root_id);
        self.add_reactions_view(self.thread_root_id);
        self.lookup_scratch.reserve(self.views.len());
        self.index.reserve_for_views(self.views.len());
    }

    pub(crate) fn lookup_into(&mut self, event: &Event) {
        self.index.lookup_into(event, &mut self.lookup_scratch);
    }

    pub(crate) fn hit_count(&self) -> usize {
        self.lookup_scratch.len()
    }

    pub(crate) fn apply_event(
        &mut self,
        event: &Event,
        delta_buffer: &mut DeltaBuffer,
    ) -> ProcessedEvent {
        if event.kind != 0 {
            self.record_hot_event(event);
        }

        delta_buffer.resize_views(self.views.len());
        let mut raw_delta_count = 0_usize;
        for index in 0..self.lookup_scratch.len() {
            let view_id = self.lookup_scratch[index];
            let Some(view) = self.views.get_mut(view_id) else {
                continue;
            };
            if let Some(delta_kind) = view.apply(event) {
                raw_delta_count += 1;
                delta_buffer.push(view_id, delta_kind);
            }
        }

        if event.kind == 0 && event.author == 42 {
            self.profile_fanout_hits += raw_delta_count as u64;
        }
        self.thread_reply_count = self
            .views
            .iter()
            .filter_map(|view| match view {
                ViewState::Thread { replies, .. } => Some(*replies),
                _ => None,
            })
            .sum();
        self.thread_reaction_count = self
            .views
            .iter()
            .filter_map(|view| match view {
                ViewState::Thread { reactions, .. } => Some(*reactions),
                ViewState::Reactions { total, .. } => Some(*total),
                _ => None,
            })
            .sum();
        ProcessedEvent { raw_delta_count }
    }

    pub(crate) fn record_hot_event(&mut self, event: &Event) {
        self.hot_event_content_bytes = self
            .hot_event_content_bytes
            .saturating_add(event.content_len);
        self.event_id_checksum ^= event.id;

        if self.hot_events.len() < self.hot_event_limit {
            self.hot_events.push(event.clone());
            return;
        }

        if self.hot_event_limit == 0 {
            return;
        }

        let evicted = &self.hot_events[self.hot_cursor];
        self.hot_event_content_bytes = self
            .hot_event_content_bytes
            .saturating_sub(evicted.content_len);
        self.event_id_checksum ^= evicted.id;
        self.hot_events[self.hot_cursor] = event.clone();
        self.hot_cursor = (self.hot_cursor + 1) % self.hot_event_limit;
    }

    pub(crate) fn next_event(&mut self, kind: StreamKind, index: usize, rng: &mut Lcg) -> Event {
        match kind {
            StreamKind::Mixed => self.synthetic_mixed_event(index, rng),
            StreamKind::Hashtag => Event {
                id: self.take_event_id(),
                kind: 1,
                author: rng.next_mod(self.author_count as u64) as u32,
                e_tags: SmallTags::empty(),
                p_tags: SmallTags::empty(),
                d_tag: None,
                hashtag_nostr: true,
                content_len: 140,
            },
            StreamKind::ProfileForSharedAuthor => {
                if index.is_multiple_of(50) {
                    Event {
                        id: self.take_event_id(),
                        kind: 0,
                        author: 42,
                        e_tags: SmallTags::empty(),
                        p_tags: SmallTags::empty(),
                        d_tag: None,
                        hashtag_nostr: false,
                        content_len: 90,
                    }
                } else {
                    Event {
                        id: self.take_event_id(),
                        kind: 1,
                        author: rng.next_mod(self.author_count as u64) as u32,
                        e_tags: SmallTags::empty(),
                        p_tags: SmallTags::empty(),
                        d_tag: None,
                        hashtag_nostr: false,
                        content_len: 120,
                    }
                }
            }
            StreamKind::ThreadEvents => {
                let is_reaction = !index.is_multiple_of(11);
                Event {
                    id: self.take_event_id(),
                    kind: if is_reaction { 7 } else { 1 },
                    author: rng.next_mod(self.author_count as u64) as u32,
                    e_tags: SmallTags::one(self.thread_root_id),
                    p_tags: SmallTags::empty(),
                    d_tag: None,
                    hashtag_nostr: false,
                    content_len: if is_reaction { 1 } else { 180 },
                }
            }
            StreamKind::AccountSwitch => {
                self.open_account_views((index % 10) as u32);
                Event {
                    id: self.take_event_id(),
                    kind: 1,
                    author: index as u32 % self.author_count,
                    e_tags: SmallTags::empty(),
                    p_tags: SmallTags::empty(),
                    d_tag: None,
                    hashtag_nostr: false,
                    content_len: 100,
                }
            }
        }
    }

    pub(crate) fn synthetic_mixed_event(&mut self, index: usize, rng: &mut Lcg) -> Event {
        let roll = rng.next_mod(100);
        let author = rng.next_mod(self.author_count as u64) as u32;
        let kind = match roll {
            0..=4 => 0,
            5..=9 => 7,
            10..=11 => 6,
            _ => 1,
        };
        let mut e_tags = SmallTags::empty();
        if kind == 7 || (index.is_multiple_of(23) && index > 0) {
            e_tags.push((index % 10_000) as u32);
        }
        Event {
            id: self.take_event_id(),
            kind,
            author,
            e_tags,
            p_tags: SmallTags::empty(),
            d_tag: if kind == 30023 {
                Some(index as u32)
            } else {
                None
            },
            hashtag_nostr: index.is_multiple_of(17),
            content_len: 80 + (rng.next_mod(200) as usize),
        }
    }

    pub(crate) fn take_event_id(&mut self) -> u64 {
        let id = self.next_event_id;
        self.next_event_id += 1;
        id
    }

    pub(crate) fn estimated_working_set_memory_bytes(&self) -> usize {
        let hot_event_bodies =
            self.hot_events.capacity() * size_of::<Event>() + self.hot_event_content_bytes;
        let cold_index_summary = self.cached_events * estimated_cold_index_entry_bytes();
        hot_event_bodies
            + cold_index_summary
            + self.views.capacity() * size_of::<ViewState>()
            + self.lookup_scratch.capacity() * size_of::<usize>()
            + self.index.estimated_memory_bytes()
    }
}

pub(crate) fn estimated_cold_index_entry_bytes() -> usize {
    16
}

pub(crate) struct ProcessedEvent {
    pub(crate) raw_delta_count: usize,
}

pub(crate) enum ViewState {
    Profile {
        author: u32,
        updates: u64,
    },
    Timeline {
        authors: Vec<u32>,
        hashtag: bool,
        items: usize,
        updates: u64,
    },
    Thread {
        root_id: u32,
        replies: u64,
        reactions: u64,
    },
    Reactions {
        target_id: u32,
        total: u64,
    },
}

impl ViewState {
    pub(crate) fn apply(&mut self, event: &Event) -> Option<PendingDeltaKind> {
        match self {
            ViewState::Profile { author, updates } => {
                if event.kind == 0 && event.author == *author {
                    *updates += 1;
                    Some(PendingDeltaKind::ProfileReplace)
                } else {
                    None
                }
            }
            ViewState::Timeline {
                authors,
                hashtag,
                items,
                updates,
            } => {
                if event.kind == 0 {
                    if authors.contains(&event.author) {
                        *updates += 1;
                        return Some(PendingDeltaKind::TimelineAuthorPatch);
                    }
                    return None;
                }

                let author_matches = authors.is_empty() || authors.contains(&event.author);
                let hashtag_matches = !*hashtag || event.hashtag_nostr;
                if author_matches && hashtag_matches && matches!(event.kind, 1 | 6 | 7) {
                    *items = (*items + 1).min(500);
                    *updates += 1;
                    Some(PendingDeltaKind::TimelineInsert)
                } else {
                    None
                }
            }
            ViewState::Thread {
                root_id,
                replies,
                reactions,
            } => {
                if event.e_tags.iter().any(|tag| tag == *root_id) {
                    if event.kind == 7 {
                        *reactions += 1;
                        Some(PendingDeltaKind::ThreadReaction)
                    } else {
                        *replies += 1;
                        Some(PendingDeltaKind::ThreadReply)
                    }
                } else {
                    None
                }
            }
            ViewState::Reactions { target_id, total } => {
                if event.kind == 7 && event.e_tags.iter().any(|tag| tag == *target_id) {
                    *total += 1;
                    Some(PendingDeltaKind::ReactionsAdjusted)
                } else {
                    None
                }
            }
        }
    }
}

pub(crate) fn author_window(start: usize, count: usize, author_count: u32) -> Vec<u32> {
    (0..count)
        .map(|offset| ((start + offset) as u32) % author_count)
        .collect()
}
