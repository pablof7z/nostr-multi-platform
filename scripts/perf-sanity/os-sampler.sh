#!/usr/bin/env bash
# os-sampler.sh — OS-level CPU%/RSS/per-thread sampler for the sanity harness.
#
# The in-process Rust bin (`sanity-gate`) cannot read its own CPU% or per-thread
# scheduling honestly (a busy-spin in the actor thread would also starve the
# sampler). So this sidecar samples an EXTERNAL pid with named OS tools and
# writes a per-phase JSON object the Rust bin merges via `--os-metrics`.
#
# Tools used (exact, named): `ps -o %cpu,rss`, `top -H`/`ps -M` (per-thread),
# `/usr/bin/time -l` (peak — captured by the caller around the whole process).
#
# Usage:
#   os-sampler.sh <pid> <phase> <duration_secs> <interval_secs> <out_json>
#
# Appends/merges `{ "<phase>": { cpu_pct_mean, cpu_pct_peak, max_thread_cpu_pct,
# rss_peak_mb, rss_slope_mb_per_hr } }` into <out_json>.
set -euo pipefail

PID="${1:?pid}"; PHASE="${2:?phase}"; DURATION="${3:?duration}"; INTERVAL="${4:-1}"; OUT="${5:?out json}"

OS="$(uname -s)"
samples_cpu=(); samples_rss=(); samples_thr=(); ts=()
start=$(date +%s)
end=$(( start + DURATION ))

# Per-thread CPU sampler — macOS `ps -M`, Linux `top -H -b`.
max_thread_cpu() {
  local pid="$1"
  if [[ "$OS" == "Darwin" ]]; then
    # ps -M: header is row 1; row 2 is the process row (USER PID TT %CPU …,
    # %CPU = $4); subsequent thread rows are indented with a blank USER field so
    # %CPU shifts to $3. Take the max across both shapes (skip the header only).
    ps -M -p "$pid" 2>/dev/null | awk '
      NR==1 {next}
      NR==2 {print $4; next}
      {print $3}' | sort -rn | head -1
  else
    top -H -b -n 1 -p "$pid" 2>/dev/null | awk -v p="$pid" '
      $1 ~ /^[0-9]+$/ {print $9}' | sort -rn | head -1
  fi
}

while [[ "$(date +%s)" -lt "$end" ]]; do
  if ! kill -0 "$PID" 2>/dev/null; then break; fi
  line=$(ps -o %cpu=,rss= -p "$PID" 2>/dev/null | head -1 || true)
  cpu=$(echo "$line" | awk '{print $1}'); rss_kb=$(echo "$line" | awk '{print $2}')
  thr=$(max_thread_cpu "$PID" || echo 0)
  [[ -z "$cpu" ]] && cpu=0
  [[ -z "$rss_kb" ]] && rss_kb=0
  [[ -z "$thr" ]] && thr=0
  samples_cpu+=("$cpu"); samples_rss+=("$rss_kb"); samples_thr+=("$thr")
  ts+=("$(date +%s)")
  sleep "$INTERVAL"
done

# If the phase window closed before any sample (process already exited), write a
# no-data marker so the merge step does not choke under `set -u`.
if [[ "${#samples_cpu[@]}" -eq 0 ]]; then
  echo "os-sampler: no samples for phase '$PHASE' (process exited before window)" >&2
  CPU_MEAN=0; CPU_PEAK=0; THR_PEAK=0; RSS_PEAK_MB=0; RSS_SLOPE=0
else
# Reduce in awk (mean/peak; least-squares slope of RSS-MB vs hours).
read -r CPU_MEAN CPU_PEAK THR_PEAK RSS_PEAK_MB RSS_SLOPE <<EOF
$(printf '%s\n' "${samples_cpu[@]}" | awk -v c="${samples_cpu[*]}" -v r="${samples_rss[*]}" \
    -v t="${samples_thr[*]}" -v tsv="${ts[*]}" '
BEGIN {
  n=split(c,ca," "); split(r,ra," "); split(t,ta," "); split(tsv,tsa," ");
  cpu_sum=0; cpu_peak=0; thr_peak=0; rss_peak=0;
  for (i=1;i<=n;i++){
    cpu_sum+=ca[i]; if(ca[i]>cpu_peak)cpu_peak=ca[i];
    if(ta[i]>thr_peak)thr_peak=ta[i];
    rmb=ra[i]/1024.0; if(rmb>rss_peak)rss_peak=rmb;
  }
  # least-squares slope of rss_mb over hours
  if(n>1){
    t0=tsa[1]; sx=0;sy=0;sxx=0;sxy=0;
    for(i=1;i<=n;i++){ x=(tsa[i]-t0)/3600.0; y=ra[i]/1024.0; sx+=x;sy+=y;sxx+=x*x;sxy+=x*y; }
    denom=(n*sxx - sx*sx);
    slope=(denom!=0)?(n*sxy - sx*sy)/denom:0;
  } else slope=0;
  printf "%.3f %.3f %.3f %.3f %.3f", (n?cpu_sum/n:0), cpu_peak, thr_peak, rss_peak, slope;
}')
EOF
fi

# Merge into OUT (create if absent) using python for safe JSON edit.
python3 - "$OUT" "$PHASE" "$CPU_MEAN" "$CPU_PEAK" "$THR_PEAK" "$RSS_PEAK_MB" "$RSS_SLOPE" <<'PY'
import json, sys, os
out, phase, mean, peak, thr, rsspeak, slope = sys.argv[1:8]
data = {}
if os.path.exists(out):
    try: data = json.load(open(out))
    except Exception: data = {}
data[phase] = {
    "cpu_pct_mean": float(mean),
    "cpu_pct_peak": float(peak),
    "max_thread_cpu_pct": float(thr),
    "rss_peak_mb": float(rsspeak),
    "rss_slope_mb_per_hr": float(slope),
}
json.dump(data, open(out, "w"), indent=2)
print(f"os-sampler: wrote phase '{phase}' -> {out}", file=sys.stderr)
PY
