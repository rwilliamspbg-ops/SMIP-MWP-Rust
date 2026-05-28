#!/usr/bin/env bash
set -euo pipefail

OUT_CSV=${OUT_CSV:-tools/bench_results/chaos_epyc_profile.csv}
OUT_REPORT=${OUT_REPORT:-benchmark/chaos_report.md}
PACKETS=${PACKETS:-10000}
PAYLOAD_LEN=${PAYLOAD_LEN:-512}
BASELINE_SEED=${BASELINE_SEED:-1337}
LOSS_PERCENT=${LOSS_PERCENT:-5}
CORRUPT_PERCENT=${CORRUPT_PERCENT:-2}
DUPLICATE_PERCENT=${DUPLICATE_PERCENT:-1}
GOAL_MAX_THROUGHPUT_DROP_PCT=${GOAL_MAX_THROUGHPUT_DROP_PCT:-5}
GOAL_MAX_P99_INCREASE_NS=${GOAL_MAX_P99_INCREASE_NS:-1000}
REPS=${REPS:-7}
AGG_METHOD=${AGG_METHOD:-median}

TMP_BASELINE=$(mktemp)
cleanup() {
  rm -f "$TMP_BASELINE"
}
trap cleanup EXIT

echo "[ci-chaos] running ideal-mode baseline"
cargo run --release -p benchmark -- \
  --packets "$PACKETS" \
  --batch-size 64 \
  --payload-len "$PAYLOAD_LEN" \
  --loss-percent 0 \
  --corrupt-percent 0 \
  --duplicate-percent 0 \
  --seed "$BASELINE_SEED" | tee "$TMP_BASELINE"

BASELINE_THROUGHPUT=$(grep -Eo 'throughput_pkt_s=[0-9.]+' "$TMP_BASELINE" | head -n1 | cut -d= -f2)
BASELINE_P99=$(grep -Eo 'latency_ns p50=[0-9]+ p99=[0-9]+' "$TMP_BASELINE" | head -n1 | sed -E 's/.*p99=([0-9]+).*/\1/')

if [[ -z "${BASELINE_THROUGHPUT}" || -z "${BASELINE_P99}" ]]; then
  echo "[ci-chaos] failed to parse baseline metrics" >&2
  exit 1
fi

echo "[ci-chaos] baseline throughput_pkt_s=${BASELINE_THROUGHPUT} p99_ns=${BASELINE_P99}"

echo "[ci-chaos] running chaos profile matrix (reps=${REPS})"

tmp_combined=$(mktemp)
echo "timestamp,core_set,packets,payload_len,loss_percent,corrupt_percent,duplicate_percent,throughput_pkt_s,p50_ns,p99_ns,p99_9_ns" > "$tmp_combined"

for i in $(seq 1 "$REPS"); do
  seed=$((BASELINE_SEED + i))
  tmp=$(mktemp)
  echo "[ci-chaos] rep=$i seed=$seed"
  PACKETS="$PACKETS" \
  PAYLOAD_LEN="$PAYLOAD_LEN" \
  LOSS_PERCENT="$LOSS_PERCENT" \
  CORRUPT_PERCENT="$CORRUPT_PERCENT" \
  DUPLICATE_PERCENT="$DUPLICATE_PERCENT" \
  CORE_SETS="0-1" \
  SEED_BASE="$seed" \
  ./tools/benchmark/run_chaos_epyc_profile.sh "$tmp"

  # pick worst p99 row from this run and append to combined (preserve CSV)
  tail -n +2 "$tmp" | awk -F',' '{
    for(i=1;i<=NF;i++){gsub(/"/,"",$i)}
    p=$10+0
    if(NR==1 || p>best_p){best_p=p; t=$1; cs=$2; packets=$3; payload=$4; loss=$5; corrupt=$6; dup=$7; thr=$8; p50=$9; p99=$10; p999=$11}
  }END{if(best_p+0>=0)printf("%s,%s,%s,%s,%s,%s,%s,%s,%s,%s,%s\n",t,cs,packets,payload,loss,corrupt,dup,thr,p50,p99,p999)}' >> "$tmp_combined"
  rm -f "$tmp"
done

mv "$tmp_combined" "$OUT_CSV"

echo "[ci-chaos] generating averaged chaos report"


# compute aggregated metrics across reps (median by default)
if [ "$AGG_METHOD" = "median" ]; then
  median_throughput=$(awk -F',' 'NR>1{print $8}' "$OUT_CSV" | sort -n | awk '{a[NR]=$1} END{ if(NR==0) print "0"; else if(NR%2==1) print a[(NR+1)/2]; else printf("%.2f", (a[NR/2]+a[(NR/2)+1])/2) }')
  median_p99=$(awk -F',' 'NR>1{print $10}' "$OUT_CSV" | sort -n | awk '{a[NR]=$1} END{ if(NR==0) print "0"; else if(NR%2==1) print a[(NR+1)/2]; else printf("%.0f", (a[NR/2]+a[(NR/2)+1])/2) }')
  agg_throughput="$median_throughput"
  agg_p99="$median_p99"
else
  # fallback to mean
  mean_throughput=$(awk -F',' 'NR>1{sum+=$8;cnt+=1}END{if(cnt>0)printf("%.2f",sum/cnt);else print "0"}' "$OUT_CSV")
  mean_p99=$(awk -F',' 'NR>1{sum+=$10;cnt+=1}END{if(cnt>0)printf("%.2f",sum/cnt);else print "0"}' "$OUT_CSV")
  agg_throughput="$mean_throughput"
  agg_p99="$mean_p99"
fi

if [ "$AGG_METHOD" = "trimmed" ]; then
  # trimmed mean: drop min and max if count>2
  agg_throughput=$(awk -F',' 'NR>1{print $8}' "$OUT_CSV" | sort -n | awk '{a[NR]=$1; s+=$1} END{ if(NR==0) print "0"; else if(NR<=2) print s/NR; else printf("%.2f", (s - a[1] - a[NR])/(NR-2)) }')
  agg_p99=$(awk -F',' 'NR>1{print $10}' "$OUT_CSV" | sort -n | awk '{a[NR]=$1; s+=$1} END{ if(NR==0) print "0"; else if(NR<=2) print int(s/NR); else printf("%.0f", (s - a[1] - a[NR])/(NR-2)) }')
fi

echo "[ci-chaos] agg_throughput=${agg_throughput} agg_p99_ns=${agg_p99} (method=${AGG_METHOD})"

throughput_drop_pct=$(awk -v b="$BASELINE_THROUGHPUT" -v a="$agg_throughput" 'BEGIN{if(b>0)printf("%.4f",((b-a)/b)*100);else print "0"}')
p99_delta_ns=$(awk -v b="$BASELINE_P99" -v a="$agg_p99" 'BEGIN{printf("%.0f",(a-b))}')

echo "[ci-chaos] Throughput drop pct=${throughput_drop_pct}% p99 delta_ns=${p99_delta_ns}"

python3 tools/benchmark/generate_chaos_report.py \
  --input "$OUT_CSV" \
  --output "$OUT_REPORT" \
  --baseline-throughput "$BASELINE_THROUGHPUT" \
  --baseline-p99-ns "$BASELINE_P99" \
  --goal-max-throughput-drop-pct "$GOAL_MAX_THROUGHPUT_DROP_PCT" \
  --goal-max-p99-increase-ns "$GOAL_MAX_P99_INCREASE_NS"

echo "[ci-chaos] report status:"
grep -E '^Status:' "$OUT_REPORT"

if [[ $(awk -v t="$throughput_drop_pct" -v g="$GOAL_MAX_THROUGHPUT_DROP_PCT" 'BEGIN{if(t+0<g+0)print 0; else print 1}') -eq 1 || $(awk -v p="$p99_delta_ns" -v gp="$GOAL_MAX_P99_INCREASE_NS" 'BEGIN{if(p+0<gp+0)print 0; else print 1}') -eq 1 ]]; then
  echo "[ci-chaos] FAIL: averaged metrics exceed envelope" >&2
  cat "$OUT_REPORT"
  exit 1
fi

echo "[ci-chaos] PASS: chaos report gate satisfied (averaged over ${REPS} reps)"
