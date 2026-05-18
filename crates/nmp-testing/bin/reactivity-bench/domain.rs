use crate::config::{DELTA_FLUSH_THRESHOLD, FLUSH_INTERVAL_NS};
use std::collections::HashMap;
use std::mem::size_of;

#[derive(Clone)]
pub(crate) struct Event {
    pub(crate) id: u64,
    pub(crate) kind: u16,
    pub(crate) author: u32,
    pub(crate) e_tags: SmallTags,
    pub(crate) p_tags: SmallTags,
    pub(crate) d_tag: Option<u32>,
    pub(crate) hashtag_nostr: bool,
    pub(crate) content_len: usize,
}

#[derive(Clone, Default)]
pub(crate) struct SmallTags {
    pub(crate) len: u8,
    pub(crate) values: [u32; 4],
}

impl SmallTags {
    pub(crate) fn empty() -> Self {
        Self::default()
    }

    pub(crate) fn one(value: u32) -> Self {
        let mut tags = Self::default();
        tags.push(value);
        tags
    }

    pub(crate) fn push(&mut self, value: u32) {
        if (self.len as usize) < self.values.len() {
            self.values[self.len as usize] = value;
            self.len += 1;
        }
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = u32> + '_ {
        self.values[..self.len as usize].iter().copied()
    }
}

#[derive(Clone)]
pub(crate) struct Dependencies {
    pub(crate) kinds: Vec<u16>,
    pub(crate) authors: Vec<u32>,
    pub(crate) e_tag_refs: Vec<u32>,
    pub(crate) p_tag_refs: Vec<u32>,
    pub(crate) d_tag_refs: Vec<u32>,
    pub(crate) kind_author_pairs: Vec<(u16, u32)>,
    pub(crate) catch_all: bool,
}

impl Dependencies {
    pub(crate) fn empty() -> Self {
        Self {
            kinds: Vec::new(),
            authors: Vec::new(),
            e_tag_refs: Vec::new(),
            p_tag_refs: Vec::new(),
            d_tag_refs: Vec::new(),
            kind_author_pairs: Vec::new(),
            catch_all: false,
        }
    }
}

pub(crate) struct ReverseIndex {
    pub(crate) by_kind_author: HashMap<(u16, u32), Vec<usize>>,
    pub(crate) by_kind_e_tag: HashMap<(u16, u32), Vec<usize>>,
    pub(crate) by_kind_p_tag: HashMap<(u16, u32), Vec<usize>>,
    pub(crate) by_kind_author_d: HashMap<(u16, u32, u32), Vec<usize>>,
    pub(crate) by_kind_d_tag: HashMap<(u16, u32), Vec<usize>>,
    pub(crate) by_kind: HashMap<u16, Vec<usize>>,
    pub(crate) by_author: HashMap<u32, Vec<usize>>,
    pub(crate) by_e_tag: HashMap<u32, Vec<usize>>,
    pub(crate) by_p_tag: HashMap<u32, Vec<usize>>,
    pub(crate) by_d_tag: HashMap<u32, Vec<usize>>,
    pub(crate) catch_all: Vec<usize>,
    pub(crate) marks: Vec<u32>,
    pub(crate) mark_generation: u32,
}

impl ReverseIndex {
    pub(crate) fn new() -> Self {
        Self {
            by_kind_author: HashMap::new(),
            by_kind_e_tag: HashMap::new(),
            by_kind_p_tag: HashMap::new(),
            by_kind_author_d: HashMap::new(),
            by_kind_d_tag: HashMap::new(),
            by_kind: HashMap::new(),
            by_author: HashMap::new(),
            by_e_tag: HashMap::new(),
            by_p_tag: HashMap::new(),
            by_d_tag: HashMap::new(),
            catch_all: Vec::new(),
            marks: Vec::new(),
            mark_generation: 0,
        }
    }

    pub(crate) fn register(&mut self, view_id: usize, deps: &Dependencies) {
        if self.marks.len() <= view_id {
            self.marks.resize(view_id + 1, 0);
        }

        for key in &deps.kind_author_pairs {
            self.by_kind_author.entry(*key).or_default().push(view_id);
        }

        if !deps.kinds.is_empty() && !deps.authors.is_empty() && !deps.d_tag_refs.is_empty() {
            for kind in &deps.kinds {
                for author in &deps.authors {
                    for d_tag in &deps.d_tag_refs {
                        self.by_kind_author_d
                            .entry((*kind, *author, *d_tag))
                            .or_default()
                            .push(view_id);
                    }
                }
            }
        } else if !deps.kinds.is_empty() && !deps.authors.is_empty() {
            for kind in &deps.kinds {
                for author in &deps.authors {
                    self.by_kind_author
                        .entry((*kind, *author))
                        .or_default()
                        .push(view_id);
                }
            }
        } else if !deps.kinds.is_empty() && !deps.e_tag_refs.is_empty() {
            for kind in &deps.kinds {
                for e_tag in &deps.e_tag_refs {
                    self.by_kind_e_tag
                        .entry((*kind, *e_tag))
                        .or_default()
                        .push(view_id);
                }
            }
        } else if !deps.kinds.is_empty() && !deps.p_tag_refs.is_empty() {
            for kind in &deps.kinds {
                for p_tag in &deps.p_tag_refs {
                    self.by_kind_p_tag
                        .entry((*kind, *p_tag))
                        .or_default()
                        .push(view_id);
                }
            }
        } else if !deps.kinds.is_empty() && !deps.d_tag_refs.is_empty() {
            for kind in &deps.kinds {
                for d_tag in &deps.d_tag_refs {
                    self.by_kind_d_tag
                        .entry((*kind, *d_tag))
                        .or_default()
                        .push(view_id);
                }
            }
        } else if !deps.kinds.is_empty() {
            for kind in &deps.kinds {
                self.by_kind.entry(*kind).or_default().push(view_id);
            }
        } else if !deps.authors.is_empty() {
            for author in &deps.authors {
                self.by_author.entry(*author).or_default().push(view_id);
            }
        } else if !deps.e_tag_refs.is_empty() {
            for e_tag in &deps.e_tag_refs {
                self.by_e_tag.entry(*e_tag).or_default().push(view_id);
            }
        } else if !deps.p_tag_refs.is_empty() {
            for p_tag in &deps.p_tag_refs {
                self.by_p_tag.entry(*p_tag).or_default().push(view_id);
            }
        } else if !deps.d_tag_refs.is_empty() {
            for d_tag in &deps.d_tag_refs {
                self.by_d_tag.entry(*d_tag).or_default().push(view_id);
            }
        }

        if deps.catch_all {
            self.catch_all.push(view_id);
        }
    }

    pub(crate) fn clear(&mut self) {
        self.by_kind_author.clear();
        self.by_kind_e_tag.clear();
        self.by_kind_p_tag.clear();
        self.by_kind_author_d.clear();
        self.by_kind_d_tag.clear();
        self.by_kind.clear();
        self.by_author.clear();
        self.by_e_tag.clear();
        self.by_p_tag.clear();
        self.by_d_tag.clear();
        self.catch_all.clear();
        self.marks.clear();
        self.mark_generation = 0;
    }

    pub(crate) fn reserve_for_views(&mut self, view_count: usize) {
        self.marks.resize(view_count, 0);
    }

    pub(crate) fn lookup_into(&mut self, event: &Event, out: &mut Vec<usize>) {
        out.clear();
        self.mark_generation = self.mark_generation.wrapping_add(1).max(1);
        if self.mark_generation == 1 {
            self.marks.fill(0);
        }

        extend_hits(
            out,
            &mut self.marks,
            self.mark_generation,
            self.by_kind_author.get(&(event.kind, event.author)),
        );
        if let Some(d_tag) = event.d_tag {
            extend_hits(
                out,
                &mut self.marks,
                self.mark_generation,
                self.by_kind_author_d
                    .get(&(event.kind, event.author, d_tag)),
            );
            extend_hits(
                out,
                &mut self.marks,
                self.mark_generation,
                self.by_kind_d_tag.get(&(event.kind, d_tag)),
            );
            extend_hits(
                out,
                &mut self.marks,
                self.mark_generation,
                self.by_d_tag.get(&d_tag),
            );
        }
        for e_tag in event.e_tags.iter() {
            extend_hits(
                out,
                &mut self.marks,
                self.mark_generation,
                self.by_kind_e_tag.get(&(event.kind, e_tag)),
            );
            extend_hits(
                out,
                &mut self.marks,
                self.mark_generation,
                self.by_e_tag.get(&e_tag),
            );
        }
        for p_tag in event.p_tags.iter() {
            extend_hits(
                out,
                &mut self.marks,
                self.mark_generation,
                self.by_kind_p_tag.get(&(event.kind, p_tag)),
            );
            extend_hits(
                out,
                &mut self.marks,
                self.mark_generation,
                self.by_p_tag.get(&p_tag),
            );
        }
        extend_hits(
            out,
            &mut self.marks,
            self.mark_generation,
            self.by_kind.get(&event.kind),
        );
        extend_hits(
            out,
            &mut self.marks,
            self.mark_generation,
            self.by_author.get(&event.author),
        );
        extend_hits(
            out,
            &mut self.marks,
            self.mark_generation,
            Some(&self.catch_all),
        );
    }

    pub(crate) fn estimated_memory_bytes(&self) -> usize {
        let bucket_count = self.by_kind_author.len()
            + self.by_kind_e_tag.len()
            + self.by_kind_p_tag.len()
            + self.by_kind_author_d.len()
            + self.by_kind_d_tag.len()
            + self.by_kind.len()
            + self.by_author.len()
            + self.by_e_tag.len()
            + self.by_p_tag.len()
            + self.by_d_tag.len();
        let entries = map_entries2(&self.by_kind_author)
            + map_entries2(&self.by_kind_e_tag)
            + map_entries2(&self.by_kind_p_tag)
            + map_entries3(&self.by_kind_author_d)
            + map_entries2(&self.by_kind_d_tag)
            + map_entries1(&self.by_kind)
            + map_entries1(&self.by_author)
            + map_entries1(&self.by_e_tag)
            + map_entries1(&self.by_p_tag)
            + map_entries1(&self.by_d_tag);

        bucket_count * 24
            + entries * size_of::<usize>()
            + self.catch_all.capacity() * size_of::<usize>()
            + self.marks.capacity() * size_of::<u32>()
    }
}

pub(crate) fn map_entries1<K>(map: &HashMap<K, Vec<usize>>) -> usize
where
    K: std::hash::Hash + Eq,
{
    map.values().map(Vec::len).sum()
}

pub(crate) fn map_entries2<K1, K2>(map: &HashMap<(K1, K2), Vec<usize>>) -> usize
where
    K1: std::hash::Hash + Eq,
    K2: std::hash::Hash + Eq,
{
    map.values().map(Vec::len).sum()
}

pub(crate) fn map_entries3<K1, K2, K3>(map: &HashMap<(K1, K2, K3), Vec<usize>>) -> usize
where
    K1: std::hash::Hash + Eq,
    K2: std::hash::Hash + Eq,
    K3: std::hash::Hash + Eq,
{
    map.values().map(Vec::len).sum()
}

pub(crate) fn extend_hits(
    scratch: &mut Vec<usize>,
    marks: &mut [u32],
    mark_generation: u32,
    hits: Option<&Vec<usize>>,
) {
    let Some(hits) = hits else {
        return;
    };
    for view_id in hits {
        if marks[*view_id] != mark_generation {
            marks[*view_id] = mark_generation;
            scratch.push(*view_id);
        }
    }
}

pub(crate) struct DeltaBuffer {
    pub(crate) pending: Vec<PendingDelta>,
    pub(crate) per_view_coalesced: Vec<u64>,
    pub(crate) last_flush_ns: u64,
    pub(crate) batches: u64,
    pub(crate) coalesced_delta_count: u64,
    pub(crate) mark_generation: u32,
    pub(crate) timeline_insert_marks: Vec<u32>,
    pub(crate) timeline_author_marks: Vec<u32>,
    pub(crate) profile_marks: Vec<u32>,
    pub(crate) thread_marks: Vec<u32>,
    pub(crate) reactions_marks: Vec<u32>,
}

impl DeltaBuffer {
    pub(crate) fn new(view_count: usize, initial_capacity: usize) -> Self {
        Self {
            pending: Vec::with_capacity(
                initial_capacity.max(view_count).max(DELTA_FLUSH_THRESHOLD),
            ),
            per_view_coalesced: vec![0; view_count],
            last_flush_ns: 0,
            batches: 0,
            coalesced_delta_count: 0,
            mark_generation: 0,
            timeline_insert_marks: vec![0; view_count],
            timeline_author_marks: vec![0; view_count],
            profile_marks: vec![0; view_count],
            thread_marks: vec![0; view_count],
            reactions_marks: vec![0; view_count],
        }
    }

    pub(crate) fn resize_views(&mut self, view_count: usize) {
        self.per_view_coalesced.resize(view_count, 0);
        self.timeline_insert_marks.resize(view_count, 0);
        self.timeline_author_marks.resize(view_count, 0);
        self.profile_marks.resize(view_count, 0);
        self.thread_marks.resize(view_count, 0);
        self.reactions_marks.resize(view_count, 0);
    }

    pub(crate) fn push(&mut self, view_id: usize, kind: PendingDeltaKind) {
        self.pending.push(PendingDelta { view_id, kind });
    }

    pub(crate) fn maybe_flush(&mut self, now_ns: u64, force: bool) {
        if self.pending.is_empty() {
            return;
        }
        if force
            || now_ns.saturating_sub(self.last_flush_ns) >= FLUSH_INTERVAL_NS
            || self.pending.len() >= DELTA_FLUSH_THRESHOLD
        {
            self.flush(now_ns);
        }
    }

    pub(crate) fn flush(&mut self, now_ns: u64) {
        self.mark_generation = self.mark_generation.wrapping_add(1).max(1);
        if self.mark_generation == 1 {
            self.timeline_insert_marks.fill(0);
            self.timeline_author_marks.fill(0);
            self.profile_marks.fill(0);
            self.thread_marks.fill(0);
            self.reactions_marks.fill(0);
        }

        let generation = self.mark_generation;
        let mut emitted = 0_u64;
        for delta in &self.pending {
            let marks = match delta.kind {
                PendingDeltaKind::TimelineInsert => &mut self.timeline_insert_marks,
                PendingDeltaKind::TimelineAuthorPatch => &mut self.timeline_author_marks,
                PendingDeltaKind::ProfileReplace => &mut self.profile_marks,
                PendingDeltaKind::ThreadReply | PendingDeltaKind::ThreadReaction => {
                    &mut self.thread_marks
                }
                PendingDeltaKind::ReactionsAdjusted => &mut self.reactions_marks,
            };

            if marks[delta.view_id] != generation {
                marks[delta.view_id] = generation;
                self.per_view_coalesced[delta.view_id] += 1;
                emitted += 1;
            }
        }

        self.coalesced_delta_count += emitted;
        self.pending.clear();
        self.batches += 1;
        self.last_flush_ns = now_ns;
    }
}

pub(crate) struct PendingDelta {
    pub(crate) view_id: usize,
    pub(crate) kind: PendingDeltaKind,
}

#[derive(Clone, Copy)]
pub(crate) enum PendingDeltaKind {
    TimelineInsert,
    TimelineAuthorPatch,
    ProfileReplace,
    ThreadReply,
    ThreadReaction,
    ReactionsAdjusted,
}
