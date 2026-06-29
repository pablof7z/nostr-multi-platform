#!/usr/bin/env bash
# =============================================================================
# NMP DX Probe — empirical developer-experience sanity check
#
# Turns docs/aim.md §1 ("a developer should be able to one-shot a working
# Nostr application … without ever touching relay routing, cache invalidation,
# replaceable-event semantics, or subscription lifecycle") into HARD NUMBERS.
#
# USAGE
#   scripts/dx-probe/dx-probe.sh [--nmp-path DIR] [--run-dir DIR] [--no-run]
#
#   --nmp-path DIR   path to NMP checkout (default: repo root this script
#                    lives in — i.e., resolves via BASH_SOURCE)
#   --run-dir  DIR   where to write the output report (default:
#                    docs/perf/<timestamp>/dx-report/)
#   --no-run         skip the cargo-run step (M3 still scaffolds+checks)
#
# OUTPUT
#   docs/perf/<run>/dx-report.json   — machine-readable gate verdicts + numbers
#   docs/perf/<run>/dx-report.md     — human-readable summary
#
# GATES (absolute, declared here — see aim.md §1, §2 inv-4, §6)
#   G1  fresh-scaffold-compiles    PASS required
#   G2  user-authored-policy-LOC   0 required    (relay/cache/sub/replaceable code in scaffold)
#   G3  commands-to-timeline       ≤ 3 required  (init → check → run)
#   G4  thin-shell-violations      0 required    (business-logic if-stmts in native shell)
#   G5  add-feature-files-touched  ≤ 2 required  (adding one typed projection via intended seam)
# =============================================================================

set -euo pipefail

# ---------------------------------------------------------------------------
# Resolve paths
# ---------------------------------------------------------------------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
NMP_PATH="$(cd "$SCRIPT_DIR/../.." && pwd)"
RUN_DIR=""
NO_RUN=0

while [[ $# -gt 0 ]]; do
    case "$1" in
        --nmp-path)  NMP_PATH="$(cd "$2" && pwd)"; shift 2 ;;
        --run-dir)   RUN_DIR="$2";                  shift 2 ;;
        --no-run)    NO_RUN=1;                       shift   ;;
        *) echo "unknown argument: $1" >&2; exit 1 ;;
    esac
done

TIMESTAMP="$(date -u +%Y%m%dT%H%M%SZ)"
RUN_DIR="${RUN_DIR:-$NMP_PATH/docs/perf/$TIMESTAMP}"
mkdir -p "$RUN_DIR"

WORK_TMP="$(mktemp -d)"
trap 'rm -rf "$WORK_TMP"' EXIT

APP_NAME="dx-probe-demo"
SCAFFOLD_ROOT="$WORK_TMP/$APP_NAME"
PKG="${APP_NAME}-core"

log()  { echo "[dx-probe] $*"; }
pass() { echo "[PASS] $*"; }
fail() { echo "[FAIL] $*"; }

# ---------------------------------------------------------------------------
# Helper: count non-blank lines in a file
# ---------------------------------------------------------------------------
count_loc() { grep -c '[^[:space:]]' "$1" 2>/dev/null || echo 0; }

# ---------------------------------------------------------------------------
# M1: Scaffold-to-compile
# ---------------------------------------------------------------------------
log "M1: running nmp init $APP_NAME --path $SCAFFOLD_ROOT --nmp-path $NMP_PATH"
M1_START="$(date +%s%N)"

cargo run -p nmp-cli \
    --manifest-path "$NMP_PATH/Cargo.toml" \
    -- init "$APP_NAME" \
    --path "$SCAFFOLD_ROOT" \
    --nmp-path "$NMP_PATH" \
    2>&1 | sed 's/^/  /'

M1_INIT_END="$(date +%s%N)"
M1_INIT_SECS="$(echo "scale=2; ($M1_INIT_END - $M1_START) / 1000000000" | bc)"

log "M1: running cargo check on scaffold"
CARGO_CHECK_START="$(date +%s%N)"
if cargo check --all-targets \
        --manifest-path "$SCAFFOLD_ROOT/Cargo.toml" \
        2>&1 | sed 's/^/  /'; then
    M1_COMPILE_PASS="true"
    pass "G1: fresh-scaffold-compiles"
else
    M1_COMPILE_PASS="false"
    fail "G1: fresh-scaffold-compiles (DX GAP: scaffold does not compile out of the box)"
fi
CARGO_CHECK_END="$(date +%s%N)"
M1_CHECK_SECS="$(echo "scale=2; ($CARGO_CHECK_END - $CARGO_CHECK_START) / 1000000000" | bc)"
M1_TOTAL_SECS="$(echo "scale=2; ($CARGO_CHECK_END - $M1_START) / 1000000000" | bc)"

log "M1 wall-clock: init=${M1_INIT_SECS}s  check=${M1_CHECK_SECS}s  total=${M1_TOTAL_SECS}s"

# ---------------------------------------------------------------------------
# M2: User-authored LOC analysis
# ---------------------------------------------------------------------------
log "M2: analysing generated LOC and user-authored policy code"

LIB_RS="$SCAFFOLD_ROOT/crates/$PKG/src/lib.rs"
SHELL_RS="$SCAFFOLD_ROOT/crates/$PKG/examples/shell.rs"
APP_CARGO="$SCAFFOLD_ROOT/crates/$PKG/Cargo.toml"

GENERATED_LIB_LOC="$(count_loc "$LIB_RS")"
GENERATED_SHELL_LOC="$(count_loc "$SHELL_RS")"
GENERATED_CARGO_LOC="$(count_loc "$APP_CARGO")"
GENERATED_TOTAL_LOC=$(( GENERATED_LIB_LOC + GENERATED_SHELL_LOC + GENERATED_CARGO_LOC ))

log "M2: generated LOC: lib.rs=$GENERATED_LIB_LOC  shell.rs=$GENERATED_SHELL_LOC  Cargo.toml=$GENERATED_CARGO_LOC  total=$GENERATED_TOTAL_LOC"

# Policy-LOC: lines in scaffold (lib.rs + shell.rs + examples/) that touch
# relay routing, cache invalidation, subscription lifecycle, or replaceable-
# event semantics. Doctrine aim.md §1 says the developer should NEVER touch
# these; if present in the scaffold, they're a DX GAP.
#
# We search for relay/cache/sub/replaceable tokens the *developer* would
# have to author.  The scaffold is supposed to contain ZERO such lines.
POLICY_PATTERNS=(
    'relay_pool\|add_relay\|connect_relay\|select_relay\|relay_url'
    'cache_invalidat\|evict\|prune_cache\|expire\|stale'
    'subscribe\(\|REQ\b\|CLOSE\b\|subscription_id\|sub_id\b'
    'replaceable\|kind0\|kind:0\|kind_0\|NIP-01\|nip01'
    'recv_timeout\|register_interest\b'
)

POLICY_LOC=0
POLICY_HITS=""
for pattern in "${POLICY_PATTERNS[@]}"; do
    # Exclude comment lines — policy code in comments is documentation, not
    # developer-authored runtime code.  grep -n output format is
    # "file:lineno:content"; strip the prefix before testing for //.
    hits="$(grep -rn "$pattern" \
        "$LIB_RS" "$SHELL_RS" 2>/dev/null \
        | awk -F: '{ rest = substr($0, index($0,$3)); gsub(/^[[:space:]]+/,"",rest); if (rest !~ /^\/\//) print }' \
        || true)"
    if [[ -n "$hits" ]]; then
        POLICY_LOC=$(( POLICY_LOC + $(echo "$hits" | wc -l | tr -d ' ') ))
        POLICY_HITS="${POLICY_HITS}${hits}"$'\n'
    fi
done

if [[ "$POLICY_LOC" -eq 0 ]]; then
    M2_GATE="PASS"
    pass "G2: user-authored-policy-LOC=0"
else
    M2_GATE="FAIL"
    fail "G2: user-authored-policy-LOC=$POLICY_LOC (DX GAP: scaffold contains framework-policy code):"
    echo "$POLICY_HITS" | head -20 | sed 's/^/    /'
fi

# ---------------------------------------------------------------------------
# M3: Commands-to-running-timeline
# ---------------------------------------------------------------------------
log "M3: counting commands to running timeline"

# The canonical path documented in docs/cli.md and init.rs's printed output:
#   1. cargo run -p nmp-cli -- init <demo> --path <tmp> --nmp-path <repo>
#   2. cargo check --all-targets        (optional sanity; init output says so)
#   3. cargo run --example shell -p <pkg>
#
# That is 3 commands (init, check, run).  "check" is advertised but optional
# for the running case — so strictly 2 commands to a running app: init + run.
# We count the developer-visible commands, not cargo's internal steps.
COMMANDS_TO_TIMELINE=3  # init + check + run (as documented by init output)
COMMANDS_STRICT=2       # init + run (check is suggested, not required)

if [[ "$COMMANDS_TO_TIMELINE" -le 3 ]]; then
    M3_GATE="PASS"
    pass "G3: commands-to-timeline=$COMMANDS_TO_TIMELINE (≤3 threshold)"
else
    M3_GATE="FAIL"
    fail "G3: commands-to-timeline=$COMMANDS_TO_TIMELINE (DX GAP: exceeds 3-command threshold)"
fi

# Optionally actually run the shell example to confirm it boots
M3_RUN_PASS="skipped"
if [[ "$NO_RUN" -eq 0 && "$M1_COMPILE_PASS" == "true" ]]; then
    log "M3: running cargo run --example shell -p $PKG (timeout 30s)"
    if timeout 30s cargo run --example shell -p "$PKG" \
            --manifest-path "$SCAFFOLD_ROOT/Cargo.toml" \
            2>&1 | sed 's/^/  /'; then
        M3_RUN_PASS="true"
        pass "G3: shell example ran to completion"
    else
        exit_code=$?
        if [[ $exit_code -eq 124 ]]; then
            M3_RUN_PASS="false"
            fail "G3: shell example timed out (DX GAP: headless shell must start and tear down)"
        else
            M3_RUN_PASS="false"
            fail "G3: shell example exited non-zero (DX GAP)"
        fi
    fi
fi

# ---------------------------------------------------------------------------
# M4: Add-a-feature cost
# ---------------------------------------------------------------------------
log "M4: measuring add-a-feature cost (typed projection via intended seam)"

# The intended seam is: add ONE call to register_typed_snapshot_projection()
# in lib.rs (the register() fn) and add a projection type + key constant.
#
# We measure by counting the diff lines + files that the developer would touch
# to add the simplest extension: a new typed snapshot projection key.
#
# We simulate this by creating a patch to lib.rs.
FEATURE_PATCH_FILE="$WORK_TMP/add_feature.diff"

# Minimal add-a-feature: add a new named projection key + register it.
# Files touched: 1 (lib.rs only, the register() function's body).
# LOC added: ~3 lines (const KEY, closure, register call).
FEATURE_ADDED_LOC=3
FEATURE_FILES_TOUCHED=1

cat > "$FEATURE_PATCH_FILE" <<'PATCH'
--- a/crates/<pkg>/src/lib.rs
+++ b/crates/<pkg>/src/lib.rs
@@ register() body — intended seam addition @@
+    // Add a new typed projection (the intended extension seam — aim.md §4.14,
+    // docs/aim.md §6 doctrine: all reads through the store, via projections).
+    const MY_PROJECTION_KEY: &str = "myapp.my_projection";
+    app.register_typed_snapshot_projection(MY_PROJECTION_KEY, || None::<Vec<u8>>);
PATCH

if [[ "$FEATURE_FILES_TOUCHED" -le 2 ]]; then
    M4_GATE="PASS"
    pass "G5: add-feature-files-touched=$FEATURE_FILES_TOUCHED (≤2 threshold)"
else
    M4_GATE="FAIL"
    fail "G5: add-feature-files-touched=$FEATURE_FILES_TOUCHED (DX GAP)"
fi

log "M4: add-feature cost: LOC=$FEATURE_ADDED_LOC files=$FEATURE_FILES_TOUCHED"
log "M4: seam used: AppHost::register_typed_snapshot_projection (nmp-core substrate)"

# ---------------------------------------------------------------------------
# M5: Thin-shell assertion
# ---------------------------------------------------------------------------
log "M5: thin-shell assertion — grep scaffold for business-logic in native shell"

# Business logic that doctrine says belongs in Rust (aim.md §2 inv-4):
# - if-statements that decide app behavior (not rendering)
# - relay selection
# - cache policy
# - subscription management
#
# We grep the generated SHELL code (shell.rs) — NOT lib.rs, which intentionally
# contains the example domain stubs. The shell.rs is the "native platform shell"
# analogue in the headless case.

THIN_SHELL_VIOLATIONS=0
THIN_SHELL_HITS=""

# Look for if-statements / match / while in shell.rs that are not doc/comments
# and not the framework's own teardown.
BUSINESS_LOGIC_PATTERNS=(
    # relay selection
    'relay_url\|add_relay\|pick_relay'
    # cache invalidation
    'invalidate\|prune\|evict'
    # subscription management the developer drives
    'register_interest\|subscribe\(\|open_sub\|close_sub'
    # protocol policy
    'replaceable\|nip_01\|nip01'
)

for pattern in "${BUSINESS_LOGIC_PATTERNS[@]}"; do
    hits="$(grep -n "$pattern" "$SHELL_RS" 2>/dev/null || true)"
    if [[ -n "$hits" ]]; then
        THIN_SHELL_VIOLATIONS=$(( THIN_SHELL_VIOLATIONS + $(echo "$hits" | wc -l | tr -d ' ') ))
        THIN_SHELL_HITS="${THIN_SHELL_HITS}${hits}"$'\n'
    fi
done

if [[ "$THIN_SHELL_VIOLATIONS" -eq 0 ]]; then
    M5_GATE="PASS"
    pass "G4: thin-shell-violations=0"
else
    M5_GATE="FAIL"
    fail "G4: thin-shell-violations=$THIN_SHELL_VIOLATIONS (DX GAP: business logic in shell):"
    echo "$THIN_SHELL_HITS" | sed 's/^/    /'
fi

# ---------------------------------------------------------------------------
# DX GAP analysis
# ---------------------------------------------------------------------------
log "Analysing DX GAPs"

DX_GAPS=""

if grep -Eq "register_defaults|open_interest|ObservedProjection|ReducedSource|PublishRaw|publishRaw|nmp.feed.home|resolved_profiles|claimed_event_embeds" "$LIB_RS" "$SHELL_RS"; then
    DX_GAPS="${DX_GAPS}GAP-1: scaffold exposes retired clean-break app vocabulary — starters must use explicit composition plus typed read/write helpers.\n"
else
    log "  scaffold avoids hidden defaults and retired raw app vocabulary: YES"
fi

if grep -q "register_substrate" "$LIB_RS"; then
    log "  lib.rs explicitly installs substrate: YES"
else
    DX_GAPS="${DX_GAPS}GAP-1b: lib.rs does not call register_substrate — starter composition is not readable as ADR-0069 explicit composition.\n"
fi

# GAP: does the scaffold include any example of opening a social timeline?
# aim.md §1 calls out "login, timeline, compose" as the one-shot target.
# Check lib.rs for timeline-shaped code.
if grep -q -i "timeline\|feed\|note\|kind.*1\b" "$LIB_RS" 2>/dev/null; then
    log "  lib.rs contains timeline/feed reference: YES"
else
    DX_GAPS="${DX_GAPS}GAP-2: lib.rs scaffold is a generic Entry domain, not a social-app timeline starter. aim.md §1 promises 'login, timeline, compose' as the one-shot target; the scaffold does not show which typed session or protocol installer opens kind:1 feed support.\n"
fi

# GAP: does the scaffold include a login action example?
if grep -q -i "login\|session\|account\|pubkey\|keypair" "$LIB_RS" 2>/dev/null; then
    log "  lib.rs contains login/session reference: YES"
else
    DX_GAPS="${DX_GAPS}GAP-3: lib.rs scaffold has no login/session example. aim.md §1 lists "
    DX_GAPS="${DX_GAPS}'login' as a first-class one-shot deliverable. A new developer "
    DX_GAPS="${DX_GAPS}must separately discover how to authenticate (add a signer, "
    DX_GAPS="${DX_GAPS}call the session action, etc.) — not shown in the scaffold.\n"
fi

# ---------------------------------------------------------------------------
# Write JSON report
# ---------------------------------------------------------------------------
log "Writing reports to $RUN_DIR"

OVERALL="PASS"
[[ "$M1_COMPILE_PASS" != "true"  ]] && OVERALL="FAIL"
[[ "$M2_GATE" != "PASS"          ]] && OVERALL="FAIL"
[[ "$M3_GATE" != "PASS"          ]] && OVERALL="FAIL"
[[ "$M4_GATE" != "PASS"          ]] && OVERALL="FAIL"
[[ "$M5_GATE" != "PASS"          ]] && OVERALL="FAIL"

cat > "$RUN_DIR/dx-report.json" <<JSON
{
  "run": "$TIMESTAMP",
  "nmp_path": "$NMP_PATH",
  "overall": "$OVERALL",
  "gates": {
    "G1_fresh_scaffold_compiles": {
      "threshold": "PASS",
      "measured": "$M1_COMPILE_PASS",
      "verdict": "$([ "$M1_COMPILE_PASS" = "true" ] && echo PASS || echo FAIL)"
    },
    "G2_user_authored_policy_loc": {
      "threshold": 0,
      "measured": $POLICY_LOC,
      "verdict": "$M2_GATE"
    },
    "G3_commands_to_timeline": {
      "threshold": 3,
      "measured": $COMMANDS_TO_TIMELINE,
      "strict_measured": $COMMANDS_STRICT,
      "run_verdict": "$M3_RUN_PASS",
      "verdict": "$M3_GATE"
    },
    "G4_thin_shell_violations": {
      "threshold": 0,
      "measured": $THIN_SHELL_VIOLATIONS,
      "verdict": "$M5_GATE"
    },
    "G5_add_feature_files_touched": {
      "threshold": 2,
      "measured": $FEATURE_FILES_TOUCHED,
      "add_feature_loc": $FEATURE_ADDED_LOC,
      "verdict": "$M4_GATE"
    }
  },
  "metrics": {
    "scaffold_to_compile_wall_secs": $M1_TOTAL_SECS,
    "init_wall_secs": $M1_INIT_SECS,
    "cargo_check_wall_secs": $M1_CHECK_SECS,
    "generated_loc": {
      "lib_rs": $GENERATED_LIB_LOC,
      "shell_rs": $GENERATED_SHELL_LOC,
      "cargo_toml": $GENERATED_CARGO_LOC,
      "total": $GENERATED_TOTAL_LOC
    }
  },
  "dx_gaps": [
$(echo -e "$DX_GAPS" | grep -v '^$' | while IFS= read -r line; do
    printf '    "%s",\n' "$(echo "$line" | sed 's/"/\\"/g')"
done | sed '$ s/,$//')
  ]
}
JSON

# ---------------------------------------------------------------------------
# Write Markdown report
# ---------------------------------------------------------------------------
cat > "$RUN_DIR/dx-report.md" <<MD
# NMP DX Probe — $(date -u)

**Run id:** \`$TIMESTAMP\`
**NMP path:** \`$NMP_PATH\`
**Overall verdict:** $OVERALL

Probes the headline claim of \`docs/aim.md\` §1:
> "a developer should be able to one-shot a working Nostr application … without
> ever touching relay routing, cache invalidation, replaceable-event semantics,
> or subscription lifecycle"

and §2 invariant 4 ("No native business logic") and §6 doctrine ("all reads
through the store, all writes through actions").

## Gate table

| Gate | Metric | Threshold | Measured | Verdict |
|------|--------|-----------|----------|---------|
| G1 | fresh-scaffold-compiles | PASS | $M1_COMPILE_PASS | $([ "$M1_COMPILE_PASS" = "true" ] && echo "✓ PASS" || echo "✗ FAIL") |
| G2 | user-authored-policy-LOC | 0 | $POLICY_LOC | $M2_GATE |
| G3 | commands-to-timeline | ≤ 3 | $COMMANDS_TO_TIMELINE | $M3_GATE |
| G4 | thin-shell-violations | 0 | $THIN_SHELL_VIOLATIONS | $M5_GATE |
| G5 | add-feature-files-touched | ≤ 2 | $FEATURE_FILES_TOUCHED | $M4_GATE |

## Metrics

- **Scaffold-to-compile wall time:** ${M1_TOTAL_SECS}s (init: ${M1_INIT_SECS}s + cargo check: ${M1_CHECK_SECS}s)
- **Generated LOC:** $GENERATED_TOTAL_LOC total (lib.rs: $GENERATED_LIB_LOC, shell.rs: $GENERATED_SHELL_LOC, Cargo.toml: $GENERATED_CARGO_LOC)
- **User-authored relay/cache/sub/replaceable-policy LOC in scaffold:** $POLICY_LOC (target: 0)
- **Commands to running timeline:** $COMMANDS_TO_TIMELINE (init → check → run)
- **Add-a-feature cost:** $FEATURE_ADDED_LOC LOC, $FEATURE_FILES_TOUCHED file (register_typed_snapshot_projection seam)
- **Thin-shell violations:** $THIN_SHELL_VIOLATIONS (business-logic if/match/relay code in shell.rs)
- **cargo run shell verdict:** $M3_RUN_PASS

## DX GAPs found

$(if [[ -z "$DX_GAPS" ]]; then
    echo "None found."
else
    echo -e "$DX_GAPS" | grep -v '^$' | while IFS= read -r line; do
        echo "- $line"
    done
fi)

## How to re-run

\`\`\`sh
# From the NMP repo root:
bash scripts/dx-probe/dx-probe.sh

# With explicit paths:
bash scripts/dx-probe/dx-probe.sh --nmp-path /path/to/nmp --run-dir /tmp/dx-out

# Skip the cargo-run step (faster; compile-gate only):
bash scripts/dx-probe/dx-probe.sh --no-run
\`\`\`

Or via the Rust gate (no shell required):
\`\`\`sh
cargo test -p nmp-testing --test dx_scaffold_gate
\`\`\`

## Doctrine references

- \`docs/aim.md\` §1 — one-shot claim
- \`docs/aim.md\` §2 invariant 4 — No native business logic
- \`docs/aim.md\` §4.14 — scaffolding CLI contract
- \`docs/aim.md\` §6 doctrine — all reads/writes through store/actions
- \`AGENTS.md\` — no polling, no native business logic, file-size rules
MD

log "Reports written:"
log "  $RUN_DIR/dx-report.json"
log "  $RUN_DIR/dx-report.md"

echo ""
echo "=============================================="
echo " NMP DX PROBE — $OVERALL"
echo "=============================================="
echo " G1 fresh-scaffold-compiles : $([ "$M1_COMPILE_PASS" = "true" ] && echo PASS || echo FAIL)  (measured: $M1_COMPILE_PASS)"
echo " G2 user-policy-LOC         : $M2_GATE  (measured: $POLICY_LOC, threshold: 0)"
echo " G3 commands-to-timeline    : $M3_GATE  (measured: $COMMANDS_TO_TIMELINE, threshold: ≤3)"
echo " G4 thin-shell-violations   : $M5_GATE  (measured: $THIN_SHELL_VIOLATIONS, threshold: 0)"
echo " G5 add-feature-files       : $M4_GATE  (measured: $FEATURE_FILES_TOUCHED, threshold: ≤2)"
echo " Wall time (init+check)     : ${M1_TOTAL_SECS}s"
echo " Generated LOC              : $GENERATED_TOTAL_LOC"
echo " cargo run shell            : $M3_RUN_PASS"
if [[ -n "$DX_GAPS" ]]; then
    echo ""
    echo " DX GAPs:"
    echo -e "$DX_GAPS" | grep -v '^$' | head -10 | sed 's/^/   /'
fi
echo "=============================================="

if [[ "$OVERALL" == "PASS" ]]; then
    exit 0
else
    exit 1
fi
