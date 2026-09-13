#!/usr/bin/env bash
# Interleaved A/B of end-to-end `ctx run` wall time.
# A = before (vocabulary loaded after the child exits)
# B = after  (vocabulary loaded while the child runs)
# Arms alternate and swap order each round, matching the in-repo harness.
cd /tmp/ctxbench-e2e || exit 1
ITERS=${ITERS:-9}
shift_cmd=("$@")

ms() { date +%s%3N; }
run_once() { local bin="$1"; shift; local t0 t1; t0=$(ms); "$bin" run "$@" >/dev/null 2>&1; t1=$(ms); echo $((t1-t0)); }

A=(); B=()
# untimed warmup for both arms
run_once /tmp/ctx-before.exe "${shift_cmd[@]}" >/dev/null
run_once /tmp/ctx-after.exe  "${shift_cmd[@]}" >/dev/null
for ((i=0;i<ITERS;i++)); do
  if (( i % 2 == 0 )); then
    A+=($(run_once /tmp/ctx-before.exe "${shift_cmd[@]}"))
    B+=($(run_once /tmp/ctx-after.exe  "${shift_cmd[@]}"))
  else
    B+=($(run_once /tmp/ctx-after.exe  "${shift_cmd[@]}"))
    A+=($(run_once /tmp/ctx-before.exe "${shift_cmd[@]}"))
  fi
done
stats() { printf '%s\n' "$@" | sort -n | awk '{v[NR]=$1} END {printf "n=%d min=%d p50=%d max=%d", NR, v[1], v[int((NR+1)/2)], v[NR]}'; }
echo "child: ${shift_cmd[*]}"
echo "  before (serial)    $(stats "${A[@]}")"
echo "  after  (overlapped) $(stats "${B[@]}")"
