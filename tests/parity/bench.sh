#!/bin/sh
# Decode time of the tests/data streams (64x64) and of larger conformance streams, Go and Rust on the
# same inputs, as a markdown table: tests/parity/bench.sh [N]. Needs the Go oracle ($ORACLE, default
# `oracle`) and the Rust bench ($BENCH, default `bench`) on PATH; run from the repo root. When
# `oracle-v4` (Go built with GOAMD64=v4) and `bench-v4` (Rust built with -C target-cpu=x86-64-v4 and
# the false-deps features, docs/performance.md) are on PATH too, and BENCH_NO_V4 is not 1, the two
# tables are printed again for the v4 builds. The conformance streams are in /conformance (the bench
# image) and run N/50 times each; a stream that decodes on one side only fails the script, and so
# does an empty conformance corpus unless JPEGXR_BENCH_ALLOW_EMPTY_CONFORMANCE=1.
set -eu
export LC_ALL=C
n=${1:-1000}
go=${ORACLE:-oracle}
rs=${BENCH:-bench}
grep -m1 'model name' /proc/cpuinfo >&2 || true
grep -m1 '^flags' /proc/cpuinfo | tr ' ' '\n' | grep -E '^(avx2|bmi2|avx512(f|bw|vl))$' | tr '\n' ' ' >&2 || true
echo >&2
d=/tmp/streams
mkdir -p $d && cp tests/data/synth/*.jxr $d
# The conformance streams from 30 KB to 400 KB, every sixth, so a few lossy and lossless streams of
# each format are at a size where setup does not dominate.
c=/tmp/conf
mkdir -p $c
find /conformance -iname '*.jxr' -size +30k -size -400k 2>/dev/null | sort | awk 'NR % 6 == 1' | head -30 |
  while read -r f; do cp "$f" "$c/conf-$(echo "${f#/conformance/}" | tr '/ ' '__')"; done
# An empty corpus would drop the class from the table without a word: fail unless opted out.
if [ -z "$(ls "$c" 2>/dev/null)" ] && [ "${JPEGXR_BENCH_ALLOW_EMPTY_CONFORMANCE:-}" != 1 ]; then
  echo "bench: no conformance streams in /conformance (set JPEGXR_BENCH_ALLOW_EMPTY_CONFORMANCE=1 to run without)" >&2
  exit 1
fi
nc=$((n / 50)); [ "$nc" -ge 1 ] || nc=1
# Best of five runs per file and side: the machine is shared, and one run can be slow.
best() {
  awk -F'\t' '!($1 in m) || $3 < m[$1] { m[$1] = $3; b[$1] = $2 } END { for (k in m) printf "%s\t%s\t%s\n", k, b[k], m[k] }' "$@" | sort
}
# table TAG GO_ENV RUST_ENV: one table, Go ($go) and Rust ($rs) each under its own environment. The first is
# both at their default parallelism (Go: GOMAXPROCS = the CPUs; Rust: max_threads = the CPUs); the
# second is both on one thread (GOMAXPROCS=1, BENCH_THREADS=1).
table() {
  tag=$1
  for i in 1 2 3 4 5; do
    { env $2 "$go" -bench "$n" $d; env $2 "$go" -bench "$nc" $c; } >/tmp/go$tag$i.tsv
    { env $3 "$rs" $d "$n"; env $3 "$rs" $c "$nc"; } >/tmp/rs$tag$i.tsv
  done
  best /tmp/go$tag?.tsv >/tmp/go$tag.tsv
  best /tmp/rs$tag?.tsv >/tmp/rs$tag.tsv
  # Both sides must decode the same files.
  cut -f1 /tmp/go$tag.tsv >/tmp/go$tag.names; cut -f1 /tmp/rs$tag.tsv >/tmp/rs$tag.names
  diff /tmp/go$tag.names /tmp/rs$tag.names >&2 || { echo "bench: a stream decodes on one side only" >&2; exit 1; }
  # A stream that fails its warm-up decode is skipped, not timed: say how many conformance ones.
  nin=$(ls $c | wc -l)
  for side in go rs; do
    ok=$(grep -c '^conf-' /tmp/$side$tag.tsv || true)
    echo "bench: $side skipped $((nin - ok)) of $nin conformance streams ($tag)"
  done
  # name, Go out bytes, Go ns, Rust out bytes, Rust ns. ns per byte is ms per MB.
  join -t "$(printf '\t')" /tmp/go$tag.tsv /tmp/rs$tag.tsv | tee /tmp/joined$tag.tsv | awk -F'\t' '
function add(k) { b[k] += $2; g[k] += $3; r[k] += $5; c[k]++ }
{
  k = "other"
  if (match($1, /^(Bgr24|Bgr48|Gray8|Gray16|N3x16)_(lossless|lossy)/)) k = substr($1, RSTART, RLENGTH)
  else if ($1 ~ /^conf-/) k = "conformance"
  if (!(k in seen)) { seen[k] = 1; order[++m] = k }
  add(k); add("all")
}
END {
  print "| input class | files | MB out | Go ms/MB | Rust ms/MB | Rust / Go |"
  print "|---|---|---|---|---|---|"
  order[++m] = "all"
  for (i = 1; i <= m; i++) {
    k = order[i]
    if (b[k]) printf "| %s | %d | %.2f | %.3f | %.3f | %.2f |\n", k, c[k], b[k] / 1e6, g[k] / b[k], r[k] / b[k], r[k] / g[k]
  }
}'
  # Per file: the Go and Rust ns and the ratio, the worst first (stderr, for reading the log).
  awk -F'\t' '$3 > 0 { printf "%s\t%d\t%d\t%d\t%.2f\n", $1, $2, $3, $5, $5 / $3 }' /tmp/joined$tag.tsv | sort -t"$(printf '\t')" -k5 -nr | head -40 >&2
}
# tables TAG HEADING_MARK: the two tables for the binaries $go and $rs. TAG keeps the temporary
# files of a pass apart from another's.
tables() {
  echo "$2 Default parallelism: Go GOMAXPROCS=$(nproc), Rust max_threads=$(nproc)"
  echo
  table "$1default" "" ""
  echo
  echo "$2 One thread: Go GOMAXPROCS=1, Rust max_threads=1"
  echo
  table "$1one" "GOMAXPROCS=1" "BENCH_THREADS=1"
}
tables "" "####"
if [ "${BENCH_NO_V4:-}" != 1 ] && ! grep -qw avx512bw /proc/cpuinfo; then
  echo "no AVX-512 on this machine: the x86-64-v4 tables are skipped" >&2
elif [ "${BENCH_NO_V4:-}" != 1 ] && command -v oracle-v4 >/dev/null 2>&1 && command -v bench-v4 >/dev/null 2>&1; then
  go=oracle-v4
  rs=bench-v4
  echo
  echo "#### x86-64-v4 build (Go GOAMD64=v4, Rust target-cpu=x86-64-v4 + false-deps)"
  echo
  tables v4 "#####"
fi
