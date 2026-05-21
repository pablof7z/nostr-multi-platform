# Architecture Review Gate

This repository has deterministic doctrine gates for machine-checkable rules and
an AI architecture signoff gate for judgment calls that grep cannot prove.

The gate is not a mathematical proof. It is an enforceable merge contract:

1. CI fails on deterministic doctrine violations.
2. A trusted base-branch workflow reviews each PR diff for architectural drift.
3. GitHub branch protection requires that workflow before `master` can merge.

## What The AI Gate Reviews

The reviewer is instructed to fail PRs that introduce or worsen:

- native business logic that belongs in Rust;
- polling or sleep/check progress loops;
- app-specific policy inside reusable NMP crates;
- untyped publish/write doors that bypass action validation;
- nondeterministic reducer inputs that are not explicit events;
- private-event provenance loss or unsafe secret/private-payload logging;
- lossy snapshot/delta behavior.

Existing untouched debt is not a failing finding unless the PR relies on it,
extends it, weakens a guardrail, or makes it harder to remove.

## Activation

After the workflow is merged to `master`, configure one provider:

```bash
gh secret set ANTHROPIC_API_KEY --repo pablof7z/nostr-multi-platform
gh variable set ARCHITECTURE_REVIEW_PROVIDER --repo pablof7z/nostr-multi-platform --body anthropic
gh variable set ARCHITECTURE_REVIEW_MODEL --repo pablof7z/nostr-multi-platform --body '<approved-opus-model>'
```

or:

```bash
gh secret set OPENAI_API_KEY --repo pablof7z/nostr-multi-platform
gh variable set ARCHITECTURE_REVIEW_PROVIDER --repo pablof7z/nostr-multi-platform --body openai
gh variable set ARCHITECTURE_REVIEW_MODEL --repo pablof7z/nostr-multi-platform --body '<approved-codex-review-model>'
```

Then enable branch protection:

```bash
scripts/configure-master-architecture-protection.sh
```

That script sets `ARCHITECTURE_REVIEW_REQUIRED=true` and requires these checks:

- `cargo test`
- `cargo check (android-ffi)`
- `Doctrine grep gates (D0/D6/D7/D8)`
- `File-size check (300 warn / 500 hard)`
- `AI architecture signoff`

## Security Model

The workflow uses `pull_request_target`, checks out the protected base commit,
and fetches the PR head only for diff inspection. It does not execute scripts
from the PR branch. The review report records provider, model, base SHA, head
SHA, changed files, verdict, and findings, and is uploaded as a CI artifact.

Before `ARCHITECTURE_REVIEW_REQUIRED=true`, the runner can use the mock provider
for workflow validation. Once required, mock review is rejected and a configured
provider/model/API key must be present.
