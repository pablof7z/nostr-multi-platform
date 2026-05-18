#!/usr/bin/env bash
# check-file-size.sh — enforce AGENTS.md 300/500 LOC limits on hand-authored source.
#
# LOC is counted by `wc -l` (blank lines + comments included), matching AGENTS.md wording.
# Extensions checked: .rs .swift .md .ts .tsx
#
# Usage:
#   check-file-size.sh [OPTIONS]
#
# Options:
#   --changed-only     Check only staged files (for pre-commit hook).
#                      Without this flag the full tracked tree is checked (CI mode).
#   --dry-run          Report violations but exit 0 (used by smoke tests).
#   --force-include F  Always include path F even if it matches .file-size-ignore.
#                      May be repeated. Used by smoke tests to exercise the fixture.
#
# Exit codes:
#   0  all files within limits (or --dry-run)
#   1  one or more files exceed the 500-LOC hard cap

set -euo pipefail

WARN_LOC=300
HARD_LOC=500
DRY_RUN=0
CHANGED_ONLY=0
FORCE_INCLUDES=()

# ── Argument parsing ──────────────────────────────────────────────────────────
while [[ $# -gt 0 ]]; do
    case "$1" in
        --dry-run)       DRY_RUN=1; shift ;;
        --changed-only)  CHANGED_ONLY=1; shift ;;
        --force-include) FORCE_INCLUDES+=("$2"); shift 2 ;;
        --) shift; break ;;
        -*) echo "check-file-size: unknown option: $1" >&2; exit 1 ;;
        *)  break ;;
    esac
done

# ── Locate repo root (works from any worktree) ────────────────────────────────
REPO_ROOT="$(git rev-parse --show-toplevel)"
IGNORE_FILE="$REPO_ROOT/.file-size-ignore"

# ── Collect candidate files ───────────────────────────────────────────────────
collect_files() {
    if [[ $CHANGED_ONLY -eq 1 ]]; then
        # Only staged additions/modifications
        git -C "$REPO_ROOT" diff --cached --name-only --diff-filter=ACMR
    else
        # Full tracked tree (CI mode)
        git -C "$REPO_ROOT" ls-files
    fi
    # Append force-included paths (they bypass ignore rules AND are always emitted
    # even if not yet tracked by git — useful for smoke tests).
    for fi in "${FORCE_INCLUDES[@]+"${FORCE_INCLUDES[@]}"}"; do
        echo "$fi"
    done
}

# ── Load ignore patterns from .file-size-ignore ───────────────────────────────
# Each non-blank, non-comment line is a glob pattern tested against the relative path.
# Patterns with ** match path separators (bash 'case' supports this on macOS and Linux).
declare -a IGNORE_PATTERNS=()

if [[ -f "$IGNORE_FILE" ]]; then
    while IFS= read -r line; do
        # Skip blank lines and comments
        [[ -z "${line// /}" || "${line:0:1}" == "#" ]] && continue
        IGNORE_PATTERNS+=("$line")
    done < "$IGNORE_FILE"
fi

# ── Check if a relative path is ignored ──────────────────────────────────────
is_ignored() {
    local rel="$1"

    # Force-included files bypass ignore rules
    for fi in "${FORCE_INCLUDES[@]+"${FORCE_INCLUDES[@]}"}"; do
        if [[ "$rel" == "$fi" ]]; then
            return 1  # NOT ignored
        fi
    done

    # Test against each pattern
    for pat in "${IGNORE_PATTERNS[@]+"${IGNORE_PATTERNS[@]}"}"; do
        # Use case for glob matching (** supported by bash on Linux + macOS)
        # shellcheck disable=SC2254
        case "$rel" in
            $pat) return 0 ;;  # ignored
        esac
    done
    return 1  # not ignored
}

# ── Main check loop ───────────────────────────────────────────────────────────
WARNINGS=0
FAILURES=0

while IFS= read -r rel_path; do
    # Filter by extension
    case "$rel_path" in
        *.rs|*.swift|*.md|*.ts|*.tsx) ;;
        *) continue ;;
    esac

    abs_path="$REPO_ROOT/$rel_path"
    [[ -f "$abs_path" ]] || continue

    # Check ignore rules
    is_ignored "$rel_path" && continue

    loc=$(wc -l < "$abs_path")

    if [[ $loc -ge $HARD_LOC ]]; then
        echo "HARD-cap violation ($loc LOC >= $HARD_LOC): $rel_path" >&2
        FAILURES=$((FAILURES + 1))
    elif [[ $loc -ge $WARN_LOC ]]; then
        echo "SOFT-cap warning ($loc LOC >= $WARN_LOC): $rel_path" >&2
        WARNINGS=$((WARNINGS + 1))
    fi
done < <(collect_files)

# ── Summary ───────────────────────────────────────────────────────────────────
if [[ $FAILURES -gt 0 ]]; then
    echo "" >&2
    echo "file-size gate: $FAILURES hard-cap violation(s) detected." >&2
    echo "  Split file(s) into cohesive submodules (AGENTS.md: 500 LOC hard ceiling)." >&2
    echo "  Exempt generated/output files via .file-size-ignore." >&2
    if [[ $DRY_RUN -eq 1 ]]; then
        echo "  (--dry-run: exiting 0)" >&2
        exit 0
    fi
    exit 1
fi

if [[ $WARNINGS -gt 0 ]]; then
    echo "" >&2
    echo "file-size gate: $WARNINGS soft-cap warning(s)." >&2
    echo "  Consider splitting files approaching 300 LOC (AGENTS.md soft limit)." >&2
fi

exit 0
