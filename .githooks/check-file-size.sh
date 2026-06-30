#!/usr/bin/env bash
# check-file-size.sh — enforce AGENTS.md LOC limits on hand-authored files.
#
# Two limit tiers (see AGENTS.md "File size"):
#   * Source code  — 300 LOC soft warning, 500 LOC hard cap.
#   * Non-code     — 800 LOC hard cap (documentation and other non-code text:
#                    .md, .toml, .yml, .yaml). No soft tier; prose/config has a
#                    weaker cohesion constraint than code.
#
# LOC is counted by `wc -l` (blank lines + comments included), matching AGENTS.md wording.
# Extensions checked: .rs .swift .md .ts .tsx .kt .kts .java .toml .yml
# .yaml .sh .bash .zsh .mjs, plus tracked files under .githooks/, ci/, scripts/.
#
# Usage:
#   check-file-size.sh [OPTIONS]
#
# Options:
#   --changed-only     Check only staged files (for pre-commit hook).
#                      Without this flag the full tracked tree is checked (CI mode).
#   --from-ref REF     Check only files changed from REF..TO_REF.
#   --to-ref REF       Required with --from-ref.
#   --baseline-ref REF Read the hard-cap baseline from this git ref instead of
#                      FROM_REF. Use TO_REF for push events (self-consistent
#                      tree) and FROM_REF/PR-base for pull_request events
#                      (anti-cheat on baseline raises).
#   --dry-run          Report violations but exit 0 (used by smoke tests).
#   --baseline-file F  Read hard-cap baseline from F instead of .file-size-baseline.
#                      Used by smoke tests. Ratchet integrity checks this file.
#   --force-include F  Always include path F even if it matches .file-size-ignore.
#                      May be repeated. Used by smoke tests to exercise the fixture.
#
# Exit codes:
#   0  all files within limits (or --dry-run)
#   1  one or more files exceed their hard cap (500 source / 800 non-code)

set -euo pipefail

WARN_LOC=300
HARD_LOC=500
# Non-code files (docs/config) get a single, looser hard cap and no soft tier.
DOC_WARN_LOC=800
DOC_HARD_LOC=800
DRY_RUN=0
CHANGED_ONLY=0
FROM_REF=""
TO_REF=""
BASELINE_REF=""
BASELINE_FILE_OVERRIDE=""
FORCE_INCLUDES=()

# ── Argument parsing ──────────────────────────────────────────────────────────
while [[ $# -gt 0 ]]; do
    case "$1" in
        --dry-run)       DRY_RUN=1; shift ;;
        --changed-only)  CHANGED_ONLY=1; shift ;;
        --from-ref)      FROM_REF="$2"; shift 2 ;;
        --to-ref)        TO_REF="$2"; shift 2 ;;
        --baseline-ref)  BASELINE_REF="$2"; shift 2 ;;
        --baseline-file) BASELINE_FILE_OVERRIDE="$2"; shift 2 ;;
        --force-include) FORCE_INCLUDES+=("$2"); shift 2 ;;
        --) shift; break ;;
        -*) echo "check-file-size: unknown option: $1" >&2; exit 1 ;;
        *)  break ;;
    esac
done

if [[ -n "$FROM_REF" || -n "$TO_REF" ]]; then
    if [[ -z "$FROM_REF" || -z "$TO_REF" ]]; then
        echo "check-file-size: --from-ref and --to-ref must be provided together" >&2
        exit 1
    fi
    if [[ $CHANGED_ONLY -eq 1 ]]; then
        echo "check-file-size: --changed-only cannot be combined with --from-ref/--to-ref" >&2
        exit 1
    fi
fi

# ── Locate repo root (works from any worktree) ────────────────────────────────
REPO_ROOT="$(git rev-parse --show-toplevel)"
IGNORE_FILE="$REPO_ROOT/.file-size-ignore"
BASELINE_FILE="${BASELINE_FILE_OVERRIDE:-$REPO_ROOT/.file-size-baseline}"

# ── Collect candidate files ───────────────────────────────────────────────────
collect_files() {
    if [[ $CHANGED_ONLY -eq 1 ]]; then
        # Only staged additions/modifications
        git -C "$REPO_ROOT" diff --cached --name-only --diff-filter=ACMR
    elif [[ -n "$FROM_REF" && -n "$TO_REF" ]]; then
        # Verify the base ref is actually fetchable before attempting to diff.
        # A missing base ref (e.g. fetch-depth:1 checkout where the base SHA is
        # not in the local clone) causes `git diff` to fail silently when its
        # output is consumed via a process substitution, because errors inside
        # <(...) are never propagated to the outer `set -euo pipefail` context.
        # Failing loudly here is the correct behaviour — a gate that cannot
        # compute its input must not pass.
        if ! git -C "$REPO_ROOT" cat-file -e "$FROM_REF^{commit}" 2>/dev/null; then
            echo "check-file-size: base ref '$FROM_REF' is not available in this clone." >&2
            echo "  Ensure the checkout step uses fetch-depth: 0 (or fetches the base ref)." >&2
            exit 1
        fi
        # CI mode for changed files without mutating the index.
        git -C "$REPO_ROOT" diff --name-only --diff-filter=ACMR "$FROM_REF" "$TO_REF"
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

# Wrapper that materialises collect_files output into a temp file so that any
# failure inside the function (including the explicit exit above) is fatal even
# though bash process substitutions do not propagate errors to the outer shell.
collect_files_to_tmp() {
    local _tmp
    _tmp="$(mktemp)"
    if ! collect_files > "$_tmp"; then
        rm -f "$_tmp"
        exit 1
    fi
    echo "$_tmp"
}

# ── Load ignore patterns from .file-size-ignore ───────────────────────────────
# Each non-blank, non-comment line is a glob pattern tested against the relative path.
# Patterns with ** match path separators (bash 'case' supports this on macOS and Linux).
declare -a IGNORE_PATTERNS=()
declare -a BASELINE_PATHS=()
declare -a BASELINE_LOCS=()
declare -a CURRENT_BASELINE_PATHS=()
declare -a CURRENT_BASELINE_LOCS=()
declare -a CURRENT_BASELINE_REASONS=()

if [[ -f "$IGNORE_FILE" ]]; then
    while IFS= read -r line; do
        # Skip blank lines and comments
        [[ -z "${line// /}" || "${line:0:1}" == "#" ]] && continue
        IGNORE_PATTERNS+=("$line")
    done < "$IGNORE_FILE"
fi

# ── Load hard-cap baselines ──────────────────────────────────────────────────
# Format: <relative path><TAB><LOC>[<TAB>staged:<issue-or-reason>].
# Blank/comment lines are ignored.
#
# The trusted baseline blocks expansions. In ref-diff CI mode it comes from the
# PR base so a PR cannot make an over-limit file larger by raising the baseline
# in the same change.
load_trusted_baseline_from_stream() {
    while IFS=$'\t' read -r rel loc _rest; do
        [[ -z "${rel// /}" || "${rel:0:1}" == "#" ]] && continue
        [[ "$loc" =~ ^[0-9]+$ ]] || continue
        BASELINE_PATHS+=("$rel")
        BASELINE_LOCS+=("$loc")
    done
}

# The current baseline is the checkout's self-consistency contract. It may be
# lower than the trusted baseline in a PR that shrinks a file and ratchets the
# entry in the same change.
load_current_baseline_from_stream() {
    while IFS=$'\t' read -r rel loc reason _rest; do
        [[ -z "${rel// /}" || "${rel:0:1}" == "#" ]] && continue
        [[ "$loc" =~ ^[0-9]+$ ]] || continue
        CURRENT_BASELINE_PATHS+=("$rel")
        CURRENT_BASELINE_LOCS+=("$loc")
        CURRENT_BASELINE_REASONS+=("$reason")
    done
}

# Decide which git ref's baseline to trust.
#   --baseline-ref REF  explicit choice (workflow passes this).
#   otherwise           fall back to FROM_REF for ref-diff runs.
# Reading the baseline from a ref (rather than the working tree) prevents a
# change from both growing an over-limit file AND raising its own baseline in
# the same reviewable diff. The correct ref depends on the event:
#   * pull_request: read from the PR BASE (FROM_REF) — the reviewer-approved
#     starting point. A PR that grows a file must not silently raise its base.
#   * push: read from the pushed commit itself (TO_REF) — the branch already
#     contains the merge/baseline-refresh, so comparing its files against a
#     stale PREVIOUS-commit baseline produces false "expansion" failures for
#     legitimately-refreshed entries. The pushed tree must be self-consistent.
# When the chosen ref predates the baseline file entirely (e.g. the root-commit
# fallback used for a branch's first push), fall back to the working-tree
# baseline, which represents current debt.
BASELINE_SOURCE_REF="${BASELINE_REF:-$FROM_REF}"
if [[ -n "$BASELINE_SOURCE_REF" && -z "$BASELINE_FILE_OVERRIDE" ]]; then
    if git -C "$REPO_ROOT" cat-file -e "$BASELINE_SOURCE_REF:.file-size-baseline" 2>/dev/null; then
        load_trusted_baseline_from_stream < <(git -C "$REPO_ROOT" show "$BASELINE_SOURCE_REF:.file-size-baseline")
    elif [[ -f "$BASELINE_FILE" ]]; then
        load_trusted_baseline_from_stream < "$BASELINE_FILE"
    fi
elif [[ -f "$BASELINE_FILE" ]]; then
    load_trusted_baseline_from_stream < "$BASELINE_FILE"
fi

if [[ -f "$BASELINE_FILE" ]]; then
    load_current_baseline_from_stream < "$BASELINE_FILE"
fi

baseline_loc_for() {
    local rel="$1"
    local idx
    for idx in "${!BASELINE_PATHS[@]}"; do
        if [[ "${BASELINE_PATHS[$idx]}" == "$rel" ]]; then
            echo "${BASELINE_LOCS[$idx]}"
            return 0
        fi
    done
    return 1
}

add_rename_baseline_aliases() {
    [[ -n "$FROM_REF" && -n "$TO_REF" ]] || return 0

    local status old_rel new_rel old_baseline existing
    while IFS=$'\t' read -r status old_rel new_rel; do
        [[ "$status" == R* && -n "$old_rel" && -n "$new_rel" ]] || continue
        old_baseline="$(baseline_loc_for "$old_rel" || true)"
        [[ -n "$old_baseline" ]] || continue
        existing="$(baseline_loc_for "$new_rel" || true)"
        [[ -z "$existing" ]] || continue
        BASELINE_PATHS+=("$new_rel")
        BASELINE_LOCS+=("$old_baseline")
    done < <(git -C "$REPO_ROOT" diff --name-status --find-renames --diff-filter=R "$FROM_REF" "$TO_REF")
}

add_rename_baseline_aliases

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

is_checked_file() {
    local rel="$1"
    case "$rel" in
        *.rs|*.swift|*.md|*.ts|*.tsx|*.kt|*.kts|*.java|*.toml|*.yml|*.yaml|*.sh|*.bash|*.zsh|*.mjs) return 0 ;;
        .githooks/*|ci/*|scripts/*) return 0 ;;
        *) return 1 ;;
    esac
}

# Non-code = documentation and declarative config/data (no executable logic).
# These get the looser DOC_* hard cap. Classified by extension so a non-code
# file keeps the doc cap even under .githooks/, ci/, or scripts/.
is_noncode_file() {
    case "$1" in
        *.md|*.toml|*.yml|*.yaml) return 0 ;;
        *) return 1 ;;
    esac
}

# Echo "<warn> <hard>" for the given path's tier.
caps_for_file() {
    if is_noncode_file "$1"; then
        echo "$DOC_WARN_LOC $DOC_HARD_LOC"
    else
        echo "$WARN_LOC $HARD_LOC"
    fi
}

has_staged_baseline_reason() {
    local reason="$1"
    [[ "$reason" == staged:* && -n "${reason#staged:}" ]]
}

# ── Main check loop ───────────────────────────────────────────────────────────
WARNINGS=0
FAILURES=0
BASELINED=0
STAGED_BASELINE=0

# Materialise the file list into a temp file.  Using a plain process
# substitution `< <(collect_files)` would silently swallow errors from inside
# collect_files (bash does not propagate failures out of <(...) to the outer
# set -euo pipefail context).  Writing to a temp file first makes any failure
# fatal before the loop begins.
_FILES_TMP="$(collect_files_to_tmp)"
trap 'rm -f "$_FILES_TMP"' EXIT

while IFS= read -r rel_path; do
    is_checked_file "$rel_path" || continue

    abs_path="$REPO_ROOT/$rel_path"
    [[ -f "$abs_path" ]] || continue

    # Check ignore rules
    is_ignored "$rel_path" && continue

    read -r warn_loc hard_loc < <(caps_for_file "$rel_path")
    loc=$(wc -l < "$abs_path")

    if [[ $loc -ge $hard_loc ]]; then
        baseline="$(baseline_loc_for "$rel_path" || true)"
        if [[ -n "$baseline" && $loc -le $baseline ]]; then
            echo "BASELINE hard-cap debt ($loc LOC >= $hard_loc, baseline $baseline): $rel_path" >&2
            BASELINED=$((BASELINED + 1))
        elif [[ -n "$baseline" ]]; then
            echo "HARD-cap expansion ($loc LOC > baseline $baseline): $rel_path" >&2
            FAILURES=$((FAILURES + 1))
        else
            echo "HARD-cap violation ($loc LOC >= $hard_loc): $rel_path" >&2
            FAILURES=$((FAILURES + 1))
        fi
    elif [[ $loc -ge $warn_loc ]]; then
        echo "SOFT-cap warning ($loc LOC >= $warn_loc): $rel_path" >&2
        WARNINGS=$((WARNINGS + 1))
    fi
done < "$_FILES_TMP"

# Ratchet integrity for the current baseline file. This deliberately scans only
# baseline entries, not every generated/vendor artifact, and honors the same
# ignore file plus --force-include fixture override as the main gate.
for idx in "${!CURRENT_BASELINE_PATHS[@]}"; do
    rel_path="${CURRENT_BASELINE_PATHS[$idx]}"
    baseline="${CURRENT_BASELINE_LOCS[$idx]}"
    reason="${CURRENT_BASELINE_REASONS[$idx]}"

    is_checked_file "$rel_path" || continue
    is_ignored "$rel_path" && continue

    abs_path="$REPO_ROOT/$rel_path"
    if [[ ! -f "$abs_path" ]]; then
        echo "STALE baseline entry for missing file (baseline $baseline): $rel_path" >&2
        FAILURES=$((FAILURES + 1))
        continue
    fi

    read -r _warn_loc hard_loc < <(caps_for_file "$rel_path")
    loc=$(wc -l < "$abs_path")
    if [[ $loc -lt $hard_loc ]]; then
        echo "STALE baseline entry below hard cap ($loc LOC < $hard_loc, baseline $baseline): $rel_path" >&2
        FAILURES=$((FAILURES + 1))
    elif [[ $baseline -gt $loc ]]; then
        if has_staged_baseline_reason "$reason"; then
            echo "STAGED baseline ratchet debt ($loc LOC, baseline $baseline, $reason): $rel_path" >&2
            STAGED_BASELINE=$((STAGED_BASELINE + 1))
        else
            echo "STALE baseline entry above current LOC ($loc LOC, baseline $baseline): $rel_path" >&2
            FAILURES=$((FAILURES + 1))
        fi
    fi
done

# ── Summary ───────────────────────────────────────────────────────────────────
if [[ $FAILURES -gt 0 ]]; then
    echo "" >&2
    echo "file-size gate: $FAILURES hard-cap violation(s) detected." >&2
    echo "  Split file(s) into cohesive submodules (AGENTS.md: 500 LOC hard ceiling for source, 800 for non-code docs/config)." >&2
    echo "  Legacy hard-cap debt must not exceed .file-size-baseline." >&2
    echo "  Delete retired baseline entries; lower stale entries or add staged:<issue>." >&2
    echo "  Exempt generated/output files via .file-size-ignore." >&2
    if [[ $DRY_RUN -eq 1 ]]; then
        echo "  (--dry-run: exiting 0)" >&2
        exit 0
    fi
    exit 1
fi

if [[ $STAGED_BASELINE -gt 0 ]]; then
    echo "" >&2
    echo "file-size gate: $STAGED_BASELINE staged baseline ratchet item(s)." >&2
    echo "  Keep staged reasons tied to an owning issue and remove them when split work lands." >&2
fi

if [[ $BASELINED -gt 0 ]]; then
    echo "" >&2
    echo "file-size gate: $BASELINED baseline hard-cap item(s) unchanged or reduced." >&2
    echo "  Do not raise .file-size-baseline; split files and remove entries as debt is retired." >&2
fi

if [[ $WARNINGS -gt 0 ]]; then
    echo "" >&2
    echo "file-size gate: $WARNINGS soft-cap warning(s)." >&2
    echo "  Consider splitting files approaching 300 LOC (AGENTS.md soft limit)." >&2
fi

exit 0
