#!/usr/bin/env bash
set -euo pipefail

repo="${1:-pablof7z/nostr-multi-platform}"
branch="${2:-master}"

if ! gh auth status >/dev/null 2>&1; then
  echo "gh must be authenticated with admin access to ${repo}." >&2
  exit 1
fi

if ! gh secret list --repo "${repo}" | awk '{print $1}' | grep -Eq '^(ANTHROPIC_API_KEY|OPENAI_API_KEY)$'; then
  echo "Set ANTHROPIC_API_KEY or OPENAI_API_KEY as a repo secret before enabling the gate." >&2
  exit 1
fi

if ! gh variable list --repo "${repo}" | awk '{print $1}' | grep -q '^ARCHITECTURE_REVIEW_PROVIDER$'; then
  echo "Set repo variable ARCHITECTURE_REVIEW_PROVIDER to anthropic or openai first." >&2
  exit 1
fi

if ! gh variable list --repo "${repo}" | awk '{print $1}' | grep -q '^ARCHITECTURE_REVIEW_MODEL$'; then
  echo "Set repo variable ARCHITECTURE_REVIEW_MODEL to the approved review model first." >&2
  exit 1
fi

gh variable set ARCHITECTURE_REVIEW_REQUIRED --repo "${repo}" --body true

payload="$(mktemp)"
trap 'rm -f "${payload}"' EXIT
cat >"${payload}" <<'JSON'
{
  "required_status_checks": {
    "strict": true,
    "contexts": [
      "cargo test",
      "cargo check (android-ffi)",
      "Doctrine grep gates (D0/D6/D7/D8)",
      "File-size check (300 warn / 500 hard)",
      "AI architecture signoff"
    ]
  },
  "enforce_admins": true,
  "required_pull_request_reviews": {
    "dismiss_stale_reviews": true,
    "require_code_owner_reviews": false,
    "required_approving_review_count": 0
  },
  "restrictions": null,
  "required_linear_history": false,
  "allow_force_pushes": false,
  "allow_deletions": false,
  "required_conversation_resolution": true
}
JSON

gh api \
  --method PUT \
  -H "Accept: application/vnd.github+json" \
  "repos/${repo}/branches/${branch}/protection" \
  --input "${payload}"

echo "Enabled branch protection for ${repo}:${branch} with required architecture signoff."
