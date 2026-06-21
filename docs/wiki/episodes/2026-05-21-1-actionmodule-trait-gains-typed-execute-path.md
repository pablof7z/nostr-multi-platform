---
type: episode-card
date: 2026-05-21
session: 1c093fa5-0f0e-4dee-bf38-99781e763f13
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/1c093fa5-0f0e-4dee-bf38-99781e763f13.jsonl
salience: architecture
status: superseded
subjects:
  - action-module-trait
  - action-registry-execute
  - publish-module-execute
supersedes: []
related_claims: []
source_lines:
  - 3411-3431
  - 4067-4207
  - 4142-4168
captured_at: 2026-06-18T04:41:52Z
---

# Episode: ActionModule trait gains typed execute path (ADR-0027)

## Prior State

ActionModule trait had only start() (validation) and preferred_action_id(). Execution was handled via a separate executors HashMap of closures registered at construction time — hosts could not supply a typed executor post-construction.

## Trigger

ADR-0027 added execute() as a required method on ActionModule, but PublishModule (and other implementors) were missing it, causing compile failures on master. The hotfix (#247) completed the migration.

## Decision

ActionModule trait now requires execute(). ActionRegistry::execute checks has_typed_executor() first (typed path via M::execute), falling back to the closure HashMap for compatibility during migration. PublishModule::execute is fire-and-forget — it sends ActorCommand and returns Ok(()). ClosureModule returns has_typed_executor() = false to preserve the old path.

## Consequences

- All ActionModule implementors must now provide execute()
- Dual execution path exists during migration (typed vs closure HashMap)
- PublishModule::execute sends PublishSignedEvent / PublishNote / Cancel directly via the send callback — correlation_id propagated for Publish but not for PublishNote
- The old executors HashMap path is retained for ClosureModule backward compatibility

## Open Tail

- #246 (ADR-0027 full typed ActionModule migration) still open — will remove the closure HashMap path entirely
- nip17 and nip29 ActionModule impls also need execute() added (leftover stash changes detected)

## Evidence

- transcript lines 3411-3431
- transcript lines 4067-4207
- transcript lines 4142-4168

