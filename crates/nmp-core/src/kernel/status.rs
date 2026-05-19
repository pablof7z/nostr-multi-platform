use super::*;

impl Kernel {
    pub(super) fn relay_status(&self) -> RelayStatus {
        self.relay_status_for(RelayRole::Content)
    }

    pub(super) fn relay_statuses(&self) -> Vec<RelayStatus> {
        let mut statuses: Vec<RelayStatus> = RelayRole::all()
            .into_iter()
            .map(|role| self.relay_status_for(role))
            .collect();
        // Include outbox relay URLs present in wire_subs but not covered by a
        // bootstrap role (T105 — resolved per-author URLs appear here only).
        let known_urls: std::collections::HashSet<&str> =
            statuses.iter().map(|s| s.relay_url.as_str()).collect();
        let outbox_urls: std::collections::BTreeSet<String> = self
            .wire_subs
            .values()
            .map(|sub| sub.relay_url.clone())
            .filter(|url| !known_urls.contains(url.as_str()))
            .collect();
        for url in outbox_urls {
            let active_subs = self
                .wire_subs
                .values()
                .filter(|sub| {
                    sub.relay_url == url
                        && !matches!(sub.state.as_str(), "closed" | "closed_by_relay")
                })
                .count();
            let last_event_at_ms = self
                .wire_subs
                .values()
                .filter(|sub| sub.relay_url == url)
                .filter_map(|sub| self.elapsed_ms(sub.last_event_at))
                .max();
            statuses.push(RelayStatus {
                role: "outbox".to_string(),
                relay_url: url,
                connection: if active_subs > 0 {
                    "connected".to_string()
                } else {
                    "unknown".to_string()
                },
                auth: "—".to_string(),
                nip77_negentropy: "unknown".to_string(),
                active_wire_subscriptions: active_subs,
                reconnect_count: 0,
                last_connected_at_ms: None,
                last_event_at_ms,
                last_notice: None,
                last_error: None,
                bytes_rx: 0,
                bytes_tx: 0,
                denied: false,
                last_close_reason: None,
            });
        }
        statuses
    }

    /// T112 — update the NIP-77 probe state for a relay lane. Called by the
    /// actor/observer layer when the `nmp-nip77` capability probe transitions
    /// (`ProbeState::Unknown → Probing → Supported/Unsupported`).  The string
    /// key must match a `ProbeState` variant name in snake_case:
    /// `"unknown"`, `"probing"`, `"supported"`, or `"unsupported"`.
    ///
    /// `nmp-core` does not import `nmp-nip77` (D0 — cycle would form); the
    /// caller owns the translation from `ProbeState` to the key string.
    #[allow(dead_code)] // Wired in by actor observer once nmp-nip77 CapabilityCache is plumbed
    pub(crate) fn set_nip77_probe_state(&mut self, role: RelayRole, state_key: &str) {
        self.relay_mut(role).nip77_probe_state = state_key.to_string();
    }

    pub(super) fn relay_status_for(&self, role: RelayRole) -> RelayStatus {
        let relay = self.relay(role);
        RelayStatus {
            role: role.key().to_string(),
            relay_url: self.bootstrap_urls_for_role(role).first().cloned().unwrap_or_default(),
            connection: relay.connection.clone(),
            auth: relay.auth.clone(),
            nip77_negentropy: relay.nip77_probe_state.clone(),
            active_wire_subscriptions: self
                .wire_subs
                .values()
                .filter(|sub| {
                    sub.role == role && !matches!(sub.state.as_str(), "closed" | "closed_by_relay")
                })
                .count(),
            reconnect_count: relay.reconnect_count,
            last_connected_at_ms: self.elapsed_ms(relay.connected_at),
            last_event_at_ms: self.elapsed_ms(relay.last_event_at),
            last_notice: relay.last_notice.clone(),
            last_error: relay.last_error.clone(),
            bytes_rx: relay.counters.bytes_rx,
            bytes_tx: relay.counters.bytes_tx,
            denied: relay.denied,
            last_close_reason: relay.last_close_reason.clone(),
        }
    }

    pub(super) fn logical_interests(&self) -> Vec<LogicalInterestStatus> {
        let mut interests = Vec::new();
        let target_pk = self.active_account.as_deref().unwrap_or("");
        interests.push(LogicalInterestStatus {
            key: format!("Profile({})", short_hex(target_pk)),
            state: if self.profiles.contains_key(target_pk) {
                "complete".to_string()
            } else if self.relay(RelayRole::Indexer).connection == "connected" {
                "tailing".to_string()
            } else {
                "opening".to_string()
            },
            refcount: 1,
            relay_urls: self.bootstrap_urls_for_role(RelayRole::Indexer),
            cache_coverage: self.relay_list_coverage(target_pk),
            warming_until_ms: None,
        });
        interests.push(LogicalInterestStatus {
            key: "Timeline".to_string(),
            state: if !self.timeline.is_empty() {
                "tailing".to_string()
            } else if self.timeline_requested {
                "opening".to_string()
            } else {
                "backfilling".to_string()
            },
            refcount: 1,
            relay_urls: self.bootstrap_discovery_relays(),
            cache_coverage: if self.timeline_requested {
                "partial".to_string()
            } else {
                "unknown".to_string()
            },
            warming_until_ms: None,
        });
        if !self.profile_claims.is_empty() {
            let claimed_authors = self.profile_claims.keys().cloned().collect::<BTreeSet<_>>();
            let claim_count = self
                .profile_claims
                .values()
                .map(BTreeSet::len)
                .sum::<usize>();
            let loaded = claimed_authors
                .iter()
                .filter(|pubkey| self.profiles.contains_key(*pubkey))
                .count();
            let pending = claimed_authors
                .iter()
                .filter(|pubkey| self.pending_profiles.contains(*pubkey))
                .count();
            let requested = claimed_authors
                .iter()
                .filter(|pubkey| self.requested_profiles.contains(*pubkey))
                .count();
            let active_reqs = self
                .wire_subs
                .values()
                .filter(|sub| {
                    sub.id.starts_with("profile-claim-")
                        && !matches!(sub.state.as_str(), "closed" | "closed_by_relay")
                })
                .count();
            let missing = claimed_authors.len().saturating_sub(loaded);
            let state = if missing == 0 {
                "complete"
            } else if active_reqs > 0 {
                "loading"
            } else if pending > 0 {
                "queued"
            } else {
                "tailing"
            };
            interests.push(LogicalInterestStatus {
                key: format!(
                    "UIProfileClaims({claim_count} components / {} pubkeys)",
                    claimed_authors.len()
                ),
                state: state.to_string(),
                refcount: claim_count.min(u32::MAX as usize) as u32,
                relay_urls: self.bootstrap_urls_for_role(RelayRole::Indexer),
                cache_coverage: format!(
                    "{loaded}/{} loaded, {pending} pending, {requested} requested, {active_reqs} active REQs",
                    claimed_authors.len()
                ),
                warming_until_ms: None,
            });
        }
        interests.push(LogicalInterestStatus {
            key: "NetworkDiagnostics".to_string(),
            state: "tailing".to_string(),
            refcount: 1,
            relay_urls: self.bootstrap_discovery_relays(),
            cache_coverage: "local".to_string(),
            warming_until_ms: None,
        });
        if let Some(interest) = self.selected_author.as_ref() {
            let pubkey = &interest.key;
            let note_count = self.author_items(pubkey).len();
            interests.push(LogicalInterestStatus {
                key: format!("AuthorProfile({})", short_hex(pubkey)),
                state: if self.author_request_pending {
                    "queued".to_string()
                } else if note_count > 0 {
                    "tailing".to_string()
                } else {
                    "opening".to_string()
                },
                refcount: interest.refcount,
                relay_urls: self.author_interest_relays(pubkey),
                cache_coverage: if note_count > 0 {
                    format!("{note_count} notes; {}", self.relay_list_coverage(pubkey))
                } else {
                    format!("warming; {}", self.relay_list_coverage(pubkey))
                },
                warming_until_ms: None,
            });
        }
        if let Some(interest) = self.selected_thread.as_ref() {
            let event_id = &interest.key;
            let root_id = self
                .thread_root_id(event_id)
                .unwrap_or_else(|| event_id.clone());
            let item_count = self.thread_items(event_id, &root_id).len();
            interests.push(LogicalInterestStatus {
                key: format!("Thread({})", short_hex(event_id)),
                state: if self.thread_request_pending {
                    "queued".to_string()
                } else if item_count > 0 {
                    "tailing".to_string()
                } else {
                    "opening".to_string()
                },
                refcount: interest.refcount,
                relay_urls: self.bootstrap_urls_for_role(RelayRole::Content),
                cache_coverage: if item_count > 0 {
                    format!("{item_count} events")
                } else {
                    "warming".to_string()
                },
                warming_until_ms: None,
            });
        }
        if let Some(interest) = self.diagnostic_firehose.as_ref() {
            interests.push(LogicalInterestStatus {
                key: format!("DiagnosticFirehose(#{})", interest.key),
                state: if self
                    .wire_subs
                    .values()
                    .any(|sub| sub.id.starts_with("diag-firehose-") && sub.state == "live")
                {
                    "tailing".to_string()
                } else {
                    "opening".to_string()
                },
                refcount: interest.refcount,
                relay_urls: self.bootstrap_urls_for_role(RelayRole::Content),
                cache_coverage: format!("{} events", self.diagnostic_firehose_events),
                warming_until_ms: None,
            });
        }
        interests
    }

    pub(super) fn wire_subscriptions(&self) -> Vec<WireSubscriptionStatus> {
        let mut subs = self
            .wire_subs
            .values()
            .map(|sub| WireSubscriptionStatus {
                wire_id: sub.id.clone(),
                relay_url: sub.relay_url.clone(),
                filter_summary: sub.filter_summary.clone(),
                state: sub.state.clone(),
                logical_consumer_count: 1,
                events_rx: sub.events_rx,
                opened_at_ms: self.elapsed_ms(Some(sub.opened_at)).unwrap_or(0),
                last_event_at_ms: self.elapsed_ms(sub.last_event_at),
                eose_at_ms: self.elapsed_ms(sub.eose_at),
                close_reason: sub.close_reason.clone(),
            })
            .collect::<Vec<_>>();
        subs.sort_by(|a, b| a.wire_id.cmp(&b.wire_id));
        subs
    }

    pub(super) fn relay(&self, role: RelayRole) -> &RelayHealth {
        self.relays
            .get(&role)
            .expect("relay health initialized for every role") // doctrine-allow: D6 — RelayRole enum is fixed and the constructor seeds every variant; panicking here means a new role was added without updating the seed (a logic bug, not a runtime error)
    }

    pub(super) fn relay_mut(&mut self, role: RelayRole) -> &mut RelayHealth {
        // Content + Indexer are pre-initialized in Kernel::new(); Wallet is
        // lazily created on first use (not a bootstrap-spawned lane).
        self.relays.entry(role).or_default()
    }

    pub(super) fn total_counters(&self) -> Counters {
        let mut total = Counters::default();
        for relay in self.relays.values() {
            total.frames_rx = total.frames_rx.saturating_add(relay.counters.frames_rx);
            total.events_rx = total.events_rx.saturating_add(relay.counters.events_rx);
            total.eose_rx = total.eose_rx.saturating_add(relay.counters.eose_rx);
            total.notices_rx = total.notices_rx.saturating_add(relay.counters.notices_rx);
            total.closed_rx = total.closed_rx.saturating_add(relay.counters.closed_rx);
            total.bytes_rx = total.bytes_rx.saturating_add(relay.counters.bytes_rx);
            total.bytes_tx = total.bytes_tx.saturating_add(relay.counters.bytes_tx);
        }
        total
    }

    pub(super) fn relay_list_coverage(&self, pubkey: &str) -> String {
        match self.author_relay_lists.get(pubkey) {
            Some(list) => format!(
                "nip65 r{} w{} b{}",
                list.read_relays.len(),
                list.write_relays.len(),
                list.both_relays.len()
            ),
            None => "nip65 unknown".to_string(),
        }
    }

    pub(super) fn author_interest_relays(&self, pubkey: &str) -> Vec<String> {
        let mut relays = self.bootstrap_discovery_relays();
        if let Some(list) = self.author_relay_lists.get(pubkey) {
            for relay in list
                .write_relays
                .iter()
                .chain(list.both_relays.iter())
                .chain(list.read_relays.iter())
            {
                if !relays.contains(relay) {
                    relays.push(relay.clone());
                }
            }
        }
        relays
    }

    pub(super) fn estimated_store_bytes(&self) -> usize {
        let event_bytes: usize = self
            .events
            .values()
            .map(|event| {
                event.id.len()
                    + event.author.len()
                    + event.content.len()
                    + event.tags.iter().flatten().map(String::len).sum::<usize>()
                    + 72
            })
            .sum();
        let profile_bytes: usize = self
            .profiles
            .values()
            .map(|profile| {
                profile.event_id.len()
                    + profile.display.len()
                    + profile.picture_url.as_ref().map(String::len).unwrap_or(0)
                    + profile.nip05.len()
                    + profile.about.len()
                    + 96
            })
            .sum();
        event_bytes + profile_bytes + self.seed_contacts.values().map(Vec::len).sum::<usize>() * 64
    }

    pub(super) fn elapsed_ms(&self, instant: Option<Instant>) -> Option<u128> {
        let started = self.started_at?;
        Some(instant?.duration_since(started).as_millis())
    }

    pub(super) fn log(&mut self, message: impl Into<String>) {
        let stamp = now_hms();
        let line = format!("{stamp} {}", message.into());
        eprintln!("NMP_CORE {line}");
        self.logs.push_back(line);
        while self.logs.len() > 80 {
            self.logs.pop_front();
        }
    }
}

// T112 — NIP-77 probe state projection tests.
#[cfg(test)]
mod nip77_status_tests {
    use super::*;
    use crate::relay::DEFAULT_VISIBLE_LIMIT;

    #[test]
    fn t112_nip77_probe_state_projected_into_relay_status() {
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

        // Default: both bootstrap roles report "unknown".
        let statuses = kernel.relay_statuses();
        for s in &statuses {
            if s.role == "content" || s.role == "indexer" {
                assert_eq!(
                    s.nip77_negentropy, "unknown",
                    "default probe state must be 'unknown' for role {}",
                    s.role
                );
            }
        }

        // After the actor/observer calls set_nip77_probe_state, the projection
        // reflects the new state on the matching lane.
        kernel.set_nip77_probe_state(RelayRole::Content, "probing");
        let statuses = kernel.relay_statuses();
        let content_row = statuses
            .iter()
            .find(|s| s.role == "content")
            .expect("content relay row must be present");
        assert_eq!(
            content_row.nip77_negentropy, "probing",
            "relay_statuses() must reflect the updated probe state on the content lane"
        );

        // Indexer lane is unaffected.
        let indexer_row = statuses
            .iter()
            .find(|s| s.role == "indexer")
            .expect("indexer relay row must be present");
        assert_eq!(
            indexer_row.nip77_negentropy, "unknown",
            "indexer lane must remain 'unknown' after updating only the content lane"
        );

        // Terminal states round-trip correctly.
        kernel.set_nip77_probe_state(RelayRole::Content, "supported");
        let statuses = kernel.relay_statuses();
        let content_row = statuses.iter().find(|s| s.role == "content").unwrap();
        assert_eq!(content_row.nip77_negentropy, "supported");

        kernel.set_nip77_probe_state(RelayRole::Content, "unsupported");
        let statuses = kernel.relay_statuses();
        let content_row = statuses.iter().find(|s| s.role == "content").unwrap();
        assert_eq!(content_row.nip77_negentropy, "unsupported");
    }
}
