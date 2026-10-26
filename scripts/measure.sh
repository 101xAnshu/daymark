#!/usr/bin/env bash
# Measure a release binary's background footprint in an isolated profile.
#
# The run uses temporary XDG directories and a temporary HOME, so it never
# touches the developer's database, autostart entry, logs or running instance.
# It samples RSS, CPU time and voluntary context switches of every thread in
# the process, then asks that instance to quit over its own socket.
set -euo pipefail

usage() {
    cat <<'EOF'
Usage: scripts/measure.sh [options]

Start target/release/daymark --background with temporary XDG directories,
sample it for at least 60 seconds, print a small table and quit it cleanly.

Options:
  --seconds N     sample duration in seconds (default 60, minimum 60)
  --interval N    seconds between samples (default 2, minimum 1)
  --binary PATH   Daymark binary to run (default target/release/daymark)
  -h, --help      show this help
EOF
}

seconds=60
interval=2
binary=target/release/daymark

while (($# > 0)); do
    case "$1" in
        --seconds)
            [[ $# -ge 2 ]] || { echo "measure: --seconds needs a value" >&2; exit 2; }
            seconds=$2
            shift 2
            ;;
        --interval)
            [[ $# -ge 2 ]] || { echo "measure: --interval needs a value" >&2; exit 2; }
            interval=$2
            shift 2
            ;;
        --binary)
            [[ $# -ge 2 ]] || { echo "measure: --binary needs a value" >&2; exit 2; }
            binary=$2
            shift 2
            ;;
        -h | --help)
            usage
            exit 0
            ;;
        *)
            echo "measure: unknown option: $1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

if ! [[ $seconds =~ ^[0-9]+$ ]] || ((seconds < 60)); then
    echo "measure: --seconds must be an integer of at least 60" >&2
    exit 2
fi
if ! [[ $interval =~ ^[0-9]+$ ]] || ((interval < 1)); then
    echo "measure: --interval must be an integer of at least 1" >&2
    exit 2
fi
if ((interval > seconds)); then
    echo "measure: --interval must not exceed --seconds" >&2
    exit 2
fi
if [[ ! -x $binary ]]; then
    echo "measure: $binary is not executable; run cargo build --release first" >&2
    exit 2
fi

binary_dir=$(cd "$(dirname "$binary")" && pwd)
binary="$binary_dir/$(basename "$binary")"

tmp=$(mktemp -d "${TMPDIR:-/tmp}/daymark-measure.XXXXXX")
runtime="$tmp/runtime"
mkdir -p "$runtime" "$tmp/home" "$tmp/config" "$tmp/data" "$tmp/state" "$tmp/cache"
chmod 700 "$runtime"

pid=""
cleanup() {
    if [[ -n $pid ]] && kill -0 "$pid" 2>/dev/null; then
        kill "$pid" 2>/dev/null || true
        wait "$pid" 2>/dev/null || true
    fi
    rm -rf "$tmp"
}
trap cleanup EXIT INT TERM

run_env=(
    env -i
    "HOME=$tmp/home"
    "PATH=$PATH"
    "TMPDIR=$tmp"
    "XDG_RUNTIME_DIR=$runtime"
    "XDG_CONFIG_HOME=$tmp/config"
    "XDG_DATA_HOME=$tmp/data"
    "XDG_STATE_HOME=$tmp/state"
    "XDG_CACHE_HOME=$tmp/cache"
)

rss_kib() {
    awk '/^VmRSS:/ { print $2; found = 1 } END { if (!found) print 0 }' \
        "/proc/$1/status" 2>/dev/null || echo 0
}

cpu_ticks() {
    awk -F')' '{ split($2, fields, " "); print fields[12] + fields[13] }' \
        "/proc/$1/stat" 2>/dev/null || echo 0
}

wakeups_total() {
    awk '/^voluntary_ctxt_switches:/ { sum += $2 } END { print sum + 0 }' \
        /proc/"$1"/task/*/status 2>/dev/null || echo 0
}

thread_count() {
    local count=0
    local task
    for task in /proc/"$1"/task/*; do
        [[ -d $task ]] && count=$((count + 1))
    done
    echo "$count"
}

clock_hz=$(getconf CLK_TCK)
if ! [[ $clock_hz =~ ^[0-9]+$ ]] || ((clock_hz < 1)); then
    clock_hz=100
fi

"${run_env[@]}" "$binary" --background >"$tmp/stdout" 2>"$tmp/stderr" &
pid=$!

# Let startup, database creation and the first tracker pass settle.
sleep 5
if ! kill -0 "$pid" 2>/dev/null; then
    echo "measure: the background process exited during startup" >&2
    cat "$tmp/stderr" >&2 || true
    exit 1
fi

echo "Daymark background measurement"
echo "binary:   $binary"
echo "duration: ${seconds}s, sample every ${interval}s, clock ${clock_hz} Hz"
echo
printf '%-8s %12s %9s %13s %8s\n' "time(s)" "RSS(MiB)" "CPU(%)" "wakeups/s" "threads"
printf '%-8s %12s %9s %13s %8s\n' "-------" "--------" "------" "---------" "-------"

start=$(date +%s)
prev_time=$start
prev_ticks=$(cpu_ticks "$pid")
prev_wakeups=$(wakeups_total "$pid")
rss_min=""
rss_max=""
cpu_sum=0
wakeups_sum=0
rows=0

while :; do
    now=$(date +%s)
    elapsed=$((now - start))
    if ((elapsed >= seconds)); then
        break
    fi

    rss=$(rss_kib "$pid")
    ticks=$(cpu_ticks "$pid")
    wakeups=$(wakeups_total "$pid")
    threads=$(thread_count "$pid")
    delta_time=$((now - prev_time))
    delta_ticks=$((ticks - prev_ticks))
    delta_wakeups=$((wakeups - prev_wakeups))
    if ((delta_time < 1)); then
        delta_time=1
    fi
    cpu=$(awk -v t="$delta_ticks" -v h="$clock_hz" -v s="$delta_time" \
        'BEGIN { printf "%.2f", (t / h / s) * 100 }')
    rate=$(awk -v w="$delta_wakeups" -v s="$delta_time" \
        'BEGIN { printf "%.2f", w / s }')
    rss_mib=$(awk -v k="$rss" 'BEGIN { printf "%.2f", k / 1024 }')

    printf '%-8s %12s %9s %13s %8s\n' "$elapsed" "$rss_mib" "$cpu" "$rate" "$threads"
    rss_min=$rss
    rss_max=$rss
    cpu_sum=$(awk -v a="$cpu_sum" -v b="$cpu" 'BEGIN { print a + b }')
    wakeups_sum=$(awk -v a="$wakeups_sum" -v b="$rate" 'BEGIN { print a + b }')
    rows=$((rows + 1))
    prev_time=$now
    prev_ticks=$ticks
    prev_wakeups=$wakeups

    sleep "$interval"
done

avg_cpu=$(awk -v sum="$cpu_sum" -v n="$rows" 'BEGIN { printf "%.2f", n ? sum / n : 0 }')
avg_wakeups=$(awk -v sum="$wakeups_sum" -v n="$rows" 'BEGIN { printf "%.2f", n ? sum / n : 0 }')
echo
printf 'samples: %d, RSS %s-%s MiB, average CPU %s%%, average wakeups %s/s\n' \
    "$rows" \
    "$(awk -v k="$rss_min" 'BEGIN { printf "%.2f", k / 1024 }')" \
    "$(awk -v k="$rss_max" 'BEGIN { printf "%.2f", k / 1024 }')" \
    "$avg_cpu" "$avg_wakeups"

"${run_env[@]}" "$binary" --quit >/dev/null 2>&1 || true
for _ in $(seq 1 50); do
    if ! kill -0 "$pid" 2>/dev/null; then
        break
    fi
    sleep 0.1
done
if kill -0 "$pid" 2>/dev/null; then
    echo "measure: the instance did not quit; stopping it" >&2
    kill "$pid" 2>/dev/null || true
fi
wait "$pid" 2>/dev/null || true
pid=""
