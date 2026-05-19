Findings:

- `crates/nmp-core/src/planner/compiler/partition.rs:337` - `base_shape.clone()` keeps the full original `#p` set for every tagged pubkey. For `#p=[Bob, Carol]` where only Bob has known inbox relays, Bob’s relay still gets `#p=[Bob, Carol]`, leaking/overfetching Carol and weakening fail-closed. Fix: in `route_p_tags_to_inbox`, clone `base_shape` per `tagged_pk` and replace `tags["p"]` with a singleton `{tagged_pk}` before pushing. Add a mixed known/unknown multi-`#p` test.

- `crates/nmp-testing/tests/m2_p_tag_inbox_routing.rs:319` - new hand-authored test file exceeds the 300 LOC soft limit. Fix: extract shared fixture/builders or split Case A vs Case C tests.

Prior fixes: author preservation for the inbox split is correct for the covered single-`#p` case, and the Stage 3 `originating_interests` dedupe fixes the overlapping relay duplicate. Targeted test passed: `cargo test -p nmp-testing --test m2_p_tag_inbox_routing`.