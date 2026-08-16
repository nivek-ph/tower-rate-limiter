#!/usr/bin/env bash

set -euo pipefail

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)

detect_source_dirty() {
    if ! git -C "$repo_root" rev-parse --is-inside-work-tree >/dev/null 2>&1; then
        echo "not-recorded"
    elif [[ -n $(git -C "$repo_root" status --porcelain --untracked-files=normal) ]]; then
        echo "true"
    else
        echo "false"
    fi
}

base_url=${BENCH_BASE_URL:-http://127.0.0.1:3000}
threads=${BENCH_THREADS:-4}
concurrencies=${BENCH_CONCURRENCIES:-"16 64 256"}
duration=${BENCH_DURATION:-30s}
warmup_duration=${BENCH_WARMUP_DURATION:-5s}
runs=${BENCH_RUNS:-3}
many_key_space=${BENCH_KEY_SPACE:-10000}
seed=${BENCH_SEED:-1337}
topology=${BENCH_TOPOLOGY:-host}
require_clean_source=${BENCH_REQUIRE_CLEAN_SOURCE:-0}
source_commit=${BENCH_SOURCE_COMMIT:-$(git -C "$repo_root" rev-parse HEAD 2>/dev/null || echo "not-recorded")}
source_dirty=${BENCH_SOURCE_DIRTY:-$(detect_source_dirty)}
machine_details=${BENCH_MACHINE_DETAILS:-$(uname -a)}
server_build_image=${BENCH_SERVER_BUILD_IMAGE:-not-recorded}
server_rustc_version=${BENCH_SERVER_RUSTC_VERSION:-$(rustc -V 2>/dev/null || echo "not-recorded")}
redis_image=${BENCH_REDIS_IMAGE:-not-recorded}
redis_server_version=${BENCH_REDIS_SERVER_VERSION:-$(redis-server --version 2>/dev/null || echo "not-recorded")}
app_placement=${BENCH_APP_PLACEMENT:-host}
redis_placement=${BENCH_REDIS_PLACEMENT:-host}
resource_limits=${BENCH_RESOURCE_LIMITS:-not-recorded}
timestamp=$(date -u +%Y%m%dT%H%M%SZ)
output_dir=${BENCH_OUTPUT_DIR:-"$repo_root/benchmarks/output/$timestamp"}
wrk_script="$repo_root/benchmarks/wrk/keys.lua"

if [[ $require_clean_source == 1 ]]; then
    if [[ $source_commit == not-recorded || $machine_details == not-recorded ]]; then
        echo "BENCH_SOURCE_COMMIT and BENCH_MACHINE_DETAILS are required for this benchmark" >&2
        exit 1
    fi
    if [[ $source_dirty != false ]]; then
        echo "The benchmark source must be recorded as clean (BENCH_SOURCE_DIRTY=false)" >&2
        exit 1
    fi
fi

for command in wrk curl; do
    if ! command -v "$command" >/dev/null 2>&1; then
        echo "$command is required but was not found in PATH" >&2
        exit 1
    fi
done

mkdir -p "$output_dir"

{
    echo "timestamp=$timestamp"
    echo "base_url=$base_url"
    echo "threads=$threads"
    echo "concurrencies=$concurrencies"
    echo "duration=$duration"
    echo "warmup_duration=$warmup_duration"
    echo "runs=$runs"
    echo "many_key_space=$many_key_space"
    echo "seed=$seed"
    echo "topology=$topology"
    echo "source_commit=$source_commit"
    echo "source_dirty=$source_dirty"
    echo "machine_details=$machine_details"
    echo "server_build_image=$server_build_image"
    echo "server_rustc_version=$server_rustc_version"
    echo "redis_image=$redis_image"
    echo "redis_server_version=$redis_server_version"
    echo "application_placement=$app_placement"
    echo "redis_placement=$redis_placement"
    echo "resource_limits=$resource_limits"
} | tee "$output_dir/environment.txt"

curl -fsS "$base_url/info" | tee "$output_dir/server.txt"
echo

for route in baseline memory redis; do
    curl -fsS -o /dev/null "$base_url/$route"
done

summary_file="$output_dir/summary.csv"
report_file="$output_dir/report.txt"
echo "route,distribution,key_space,concurrency,run,requests,requests_per_sec,p50_us,p95_us,p99_us,connect_errors,read_errors,write_errors,status_errors,timeouts" >"$summary_file"

{
    echo "Benchmark report"
    echo "timestamp=$timestamp"
    echo "base_url=$base_url"
    echo "threads=$threads"
    echo "concurrencies=$concurrencies"
    echo "duration=$duration"
    echo "runs=$runs"
    echo
} >"$report_file"

format_microseconds() {
    awk -v microseconds="$1" 'BEGIN {
        if (microseconds >= 1000000) {
            printf "%.2fs", microseconds / 1000000
        } else if (microseconds >= 1000) {
            printf "%.2fms", microseconds / 1000
        } else {
            printf "%dus", microseconds
        }
    }'
}

report_case_count=0

run_case() {
    local route=$1
    local distribution=$2
    local key_space=$3
    local concurrency=$4
    local run_number=$5
    local name="${route}-${distribution}-c${concurrency}-run${run_number}"
    local metrics
    local latency_stats
    local p75
    local p90
    local requests
    local requests_per_sec
    local p50_us
    local p95_us
    local p99_us
    local connect_errors
    local read_errors
    local write_errors
    local status_errors
    local timeouts

    echo "Running $name"
    BENCH_KEY_SPACE=$key_space BENCH_SEED=$seed wrk \
        -t"$threads" \
        -c"$concurrency" \
        -d"$duration" \
        --latency \
        -s "$wrk_script" \
        "$base_url/$route" | tee "$output_dir/$name.txt"

    metrics=$(sed -n 's/^BENCHMARK_CSV,//p' "$output_dir/$name.txt")
    if [[ -z $metrics ]]; then
        echo "wrk did not emit benchmark metrics for $name" >&2
        exit 1
    fi
    echo "$route,$distribution,$key_space,$concurrency,$run_number,$metrics" >>"$summary_file"

    IFS=, read -r requests requests_per_sec p50_us p95_us p99_us connect_errors read_errors \
        write_errors status_errors timeouts <<<"$metrics"
    if ((connect_errors != 0 || read_errors != 0 || write_errors != 0 || status_errors != 0 || timeouts != 0)); then
        echo "Benchmark case $name recorded request errors; inspect $output_dir/$name.txt" >&2
        exit 1
    fi
    latency_stats=$(awk '/^    Latency[[:space:]]/ { print $2, $3, $4; exit }' "$output_dir/$name.txt")
    p75=$(sed -n 's/^     75%[[:space:]]*//p' "$output_dir/$name.txt" | head -n1)
    p90=$(sed -n 's/^     90%[[:space:]]*//p' "$output_dir/$name.txt" | head -n1)

    {
        if (( report_case_count > 0 )); then
            printf '\n'
        fi
        printf '[%s]\n' "$name"
        printf 'Statistics        Avg      Stdev        Max\n'
        printf 'Latency           %s\n' "$latency_stats"
        printf 'Reqs/sec          %s\n' "$requests_per_sec"
        printf 'Latency Distribution\n'
        printf '  50%%             %s\n' "$(format_microseconds "$p50_us")"
        printf '  75%%             %s\n' "$p75"
        printf '  90%%             %s\n' "$p90"
        printf '  95%%             %s\n' "$(format_microseconds "$p95_us")"
        printf '  99%%             %s\n' "$(format_microseconds "$p99_us")"
        printf 'Requests          %s\n' "$requests"
        printf 'Errors            connect=%s read=%s write=%s status=%s timeout=%s\n' \
            "$connect_errors" "$read_errors" "$write_errors" "$status_errors" "$timeouts"
    } >>"$report_file"
    report_case_count=$((report_case_count + 1))
}

for concurrency in $concurrencies; do
    BENCH_KEY_SPACE=1 BENCH_SEED=$seed wrk \
        -t"$threads" \
        -c"$concurrency" \
        -d"$warmup_duration" \
        -s "$wrk_script" \
        "$base_url/baseline" >/dev/null

    for run_number in $(seq 1 "$runs"); do
        run_case baseline none 1 "$concurrency" "$run_number"
    done

    for route in memory redis; do
        for distribution in hot many; do
            if [[ $distribution == hot ]]; then
                key_space=1
            else
                key_space=$many_key_space
            fi

            BENCH_KEY_SPACE=$key_space BENCH_SEED=$seed wrk \
                -t"$threads" \
                -c"$concurrency" \
                -d"$warmup_duration" \
                -s "$wrk_script" \
                "$base_url/$route" >/dev/null

            for run_number in $(seq 1 "$runs"); do
                run_case "$route" "$distribution" "$key_space" "$concurrency" "$run_number"
            done
        done
    done
done

echo "Raw results written to $output_dir"
echo "CSV summary written to $summary_file"
echo "Human-readable report written to $report_file"
