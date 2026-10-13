#!/usr/bin/env bash
# Runs the matching benchmarks on both cores and prints a Markdown report on
# stdout (progress and diagnostics go to stderr), so it can be piped straight
# into docs/rust-migration/benchmarks.md:
#
#   ./scripts/bench.sh | tee docs/rust-migration/benchmarks.md
#
# Programs (all built in release mode):
#   sdm-bench       Rust core        transfer, ticks
#   SDMBench        all-Swift core   transfer, ticks   (legacy/SDMKitSwift)
#   SDMFFIBench     Rust core via the generated Swift bindings   ffi-snapshot,
#                                                                ffi-snapshot-if-changed
#
# Each scenario runs RUNS times (5) under /usr/bin/time -l, the two cores
# alternating run by run. Refuses to run on battery, or with under 6 GB free
# where the transfer scenario writes its 2 GB.
#
# Every run gets TMPDIR pointed at one directory this script creates; the
# programs make their own sdm-bench-* directory inside it and delete it. The
# script removes its directory, by the exact path it recorded, on exit.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RUNS=5
MIN_FREE_KB=$((6 * 1024 * 1024))
export PATH="$HOME/.cargo/bin:$PATH"

log() { echo "bench: $*" >&2; }
die() { echo "bench: $*" >&2; exit 1; }

# --- Preconditions -----------------------------------------------------------

if ! pmset -g batt | head -1 | grep -q "AC Power"; then
    die "refusing to run on battery power; plug in first ($(pmset -g batt | head -1))"
fi

SYSTEM_TMP="$(cd "${TMPDIR:-/tmp}" && pwd -P)"
FREE_KB="$(df -k "$SYSTEM_TMP" | awk 'NR==2 {print $4}')"
if [ "$FREE_KB" -lt "$MIN_FREE_KB" ]; then
    die "only $((FREE_KB / 1024 / 1024)) GB free under $SYSTEM_TMP; the transfer scenario needs 6 GB"
fi

# --- Scratch directory (removed by exact path on exit) -----------------------

WORK="$(mktemp -d "$SYSTEM_TMP/sdm-bench-run.XXXXXX")"
case "$WORK" in
    "$SYSTEM_TMP"/sdm-bench-run.*) ;;
    *) die "unexpected scratch path $WORK" ;;
esac
cleanup() {
    if [ -n "${WORK:-}" ] && [ -d "$WORK" ]; then
        rm -rf -- "$WORK"
    fi
}
trap cleanup EXIT
RESULTS="$WORK/results.tsv"
: > "$RESULTS"

# --- Build -------------------------------------------------------------------

log "building sdm-bench (Rust)"
(cd "$ROOT/sdm-core" && cargo build --release -p sdm-bench >&2)
RUST_BENCH="$ROOT/sdm-core/target/release/sdm-bench"

log "building SDMBench (all-Swift core)"
(cd "$ROOT/legacy/SDMKitSwift" && swift build -c release --product SDMBench >&2)
SWIFT_BENCH="$ROOT/legacy/SDMKitSwift/.build/release/SDMBench"

log "building SDMFFIBench (Rust core through the bindings)"
(cd "$ROOT/SDMKit" && swift build -c release --product SDMFFIBench >&2)
FFI_BENCH="$ROOT/SDMKit/.build/release/SDMFFIBench"

for program in "$RUST_BENCH" "$SWIFT_BENCH" "$FFI_BENCH"; do
    [ -x "$program" ] || die "missing $program"
done

# --- Runs --------------------------------------------------------------------

# run_one <scenario> <core label> <program> <argument>
run_one() {
    local scenario="$1" core="$2" program="$3" argument="$4"
    local timing="$WORK/time.txt" output="$WORK/stdout.txt"
    local status=0
    TMPDIR="$WORK/" /usr/bin/time -l -o "$timing" "$program" "$argument" > "$output" || status=$?
    [ "$status" -eq 0 ] || die "$core $scenario exited with status $status"
    local line
    line="$(cat "$output")"
    case "$line" in
        "scenario=$scenario ok=true "*) ;;
        *) die "$core $scenario printed an unexpected line: $line" ;;
    esac
    local real user sys rss
    read -r real _ user _ sys _ < <(grep ' real ' "$timing")
    rss="$(awk '/maximum resident set size/ {print $1}' "$timing")"
    [ -n "$real" ] && [ -n "$user" ] && [ -n "$sys" ] && [ -n "$rss" ] || die "could not parse $timing"
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$scenario" "$core" "$real" "$user" "$sys" "$rss" "$line" >> "$RESULTS"
    log "$scenario $core: ${real}s wall, ${user}s user, ${sys}s sys, $((rss / 1048576)) MB peak  ($line)"
    # Nothing may be left behind inside the scratch directory but our files.
    if compgen -G "$WORK/sdm-bench-*" > /dev/null; then
        die "$core $scenario left a sdm-bench-* directory behind in $WORK"
    fi
}

for scenario in transfer ticks; do
    for run in $(seq 1 "$RUNS"); do
        log "$scenario, run $run of $RUNS"
        run_one "$scenario" "Rust" "$RUST_BENCH" "$scenario"
        run_one "$scenario" "Swift" "$SWIFT_BENCH" "$scenario"
    done
done
for scenario in ffi-snapshot ffi-snapshot-if-changed; do
    for run in $(seq 1 "$RUNS"); do
        log "$scenario, run $run of $RUNS"
        run_one "$scenario" "Rust via bindings" "$FFI_BENCH" "$scenario"
    done
done

# --- Report ------------------------------------------------------------------

MODEL="$(sysctl -n hw.model)"
CPU="$(sysctl -n machdep.cpu.brand_string)"
CORES="$(sysctl -n hw.ncpu)"
MEMORY_GB="$(($(sysctl -n hw.memsize) / 1073741824))"
MACOS="$(sw_vers -productVersion) ($(sw_vers -buildVersion))"
COMMIT="$(git -C "$ROOT" rev-parse --short HEAD)"

python3 -I - "$RESULTS" "$MODEL" "$CPU" "$CORES" "$MEMORY_GB" "$MACOS" "$COMMIT" "$RUNS" <<'PY'
import statistics
import sys
from datetime import date

path, model, cpu, cores, memory, macos, commit, runs = sys.argv[1:]
rows = []
with open(path) as handle:
    for line in handle:
        scenario, core, real, user, sys_time, rss, output = line.rstrip("\n").split("\t")
        fields = dict(part.split("=", 1) for part in output.split())
        rows.append(
            dict(
                scenario=scenario,
                core=core,
                real=float(real),
                cpu=float(user) + float(sys_time),
                rss=int(rss) / 1048576,
                items=int(fields["items"]),
                bytes=int(fields["bytes"]),
                ticks=int(fields["ticks"]),
            )
        )

print("# Benchmarks: Swift core and Rust core")
print()
print(f"- Date: {date.today().isoformat()}")
print(f"- Machine: {model}, {cpu}, {cores} logical cores, {memory} GB memory")
print(f"- macOS: {macos}")
print(f"- Commit: {commit}")
print(f"- Release builds, {runs} runs per cell, the two cores alternating run by run; medians below.")
print("- Wall, CPU (user + system) and peak resident memory are measured from outside with `/usr/bin/time -l`.")
print()

groups = {}
for row in rows:
    groups.setdefault((row["scenario"], row["core"]), []).append(row)

print("| Scenario | Core | Wall (s) | CPU user+sys (s) | Peak RSS (MB) | Wall spread | Items | Bytes | Ticks |")
print("|---|---|---:|---:|---:|---:|---:|---:|---:|")
for (scenario, core), group in groups.items():
    wall = statistics.median(r["real"] for r in group)
    cpu_time = statistics.median(r["cpu"] for r in group)
    rss = statistics.median(r["rss"] for r in group)
    spread = (max(r["real"] for r in group) - min(r["real"] for r in group)) / wall * 100
    flag = " (over 15%)" if spread > 15 else ""
    print(
        f"| {scenario} | {core} | {wall:.2f} | {cpu_time:.2f} | {rss:.0f} | "
        f"{spread:.1f}%{flag} | {group[0]['items']} | {group[0]['bytes']} | "
        f"{statistics.median(r['ticks'] for r in group):.0f} |"
    )
print()
print("Wall spread is (max - min) / median of the wall times over the runs.")
print()
print("## Every run")
print()
print("| Scenario | Core | Run | Wall (s) | CPU user+sys (s) | Peak RSS (MB) |")
print("|---|---|---:|---:|---:|---:|")
for (scenario, core), group in groups.items():
    for index, r in enumerate(group, 1):
        print(f"| {scenario} | {core} | {index} | {r['real']:.2f} | {r['cpu']:.2f} | {r['rss']:.0f} |")
PY
