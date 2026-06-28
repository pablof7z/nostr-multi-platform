#!/usr/bin/env bash
# check-no-review-dumps.sh — repository hygiene gate
#
# Enforces two "never recreate" constraints from AGENTS.md planning-discipline:
#
#   1. docs/perf/codex-reviews/ must contain no committed files.
#      AI code review output (codex reviews, direction reviews, post-merge
#      review dumps) must never be committed.  Actionable findings belong in
#      GitHub Issues; lasting insights belong in durable docs.  The directory
#      was explicitly retired in docs/retired/removed-documents.md.
#
#   2. docs/perf/pending-user-decisions.md must not exist.
#      The file was a historical append-only queue; it is retired.  New
#      pending decisions belong in GitHub Issues with status:decision label.
#
# See AGENTS.md §"Planning discipline" rule "Never commit code reviews."
#
# Environment:
#   REPO_ROOT  — override the repo root (default: directory containing ci/).
#                Used by the CI smoke test to validate against a temp tree.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="${REPO_ROOT:-$(cd "$SCRIPT_DIR/.." && pwd)}"

FAIL=0

# ── Gate 1: no committed files in docs/perf/codex-reviews/ ──────────────────
REVIEWS_DIR="$ROOT/docs/perf/codex-reviews"
if [[ -d "$REVIEWS_DIR" ]]; then
    review_files=$(find "$REVIEWS_DIR" -maxdepth 5 -type f 2>/dev/null)
    if [[ -n "$review_files" ]]; then
        echo "FAIL: committed review dump(s) found in docs/perf/codex-reviews/:" >&2
        while IFS= read -r f; do
            echo "  $f" >&2
        done <<< "$review_files"
        echo "" >&2
        echo "  AI code review output must not be committed.  Per AGENTS.md:" >&2
        echo "  promote actionable findings to a GitHub issue or durable doc," >&2
        echo "  then discard the review artifact." >&2
        FAIL=1
    fi
fi

# ── Gate 2: docs/perf/pending-user-decisions.md must not exist ──────────────
PUD_FILE="$ROOT/docs/perf/pending-user-decisions.md"
if [[ -f "$PUD_FILE" ]]; then
    echo "FAIL: docs/perf/pending-user-decisions.md must not be recreated." >&2
    echo "" >&2
    echo "  This file is retired (docs/retired/removed-documents.md)." >&2
    echo "  New pending decisions belong in GitHub Issues" >&2
    echo "  (label: status:decision or category:decision)." >&2
    FAIL=1
fi

if [[ $FAIL -eq 0 ]]; then
    echo "OK: no committed review dumps or retired planning surfaces detected."
fi

exit $FAIL
