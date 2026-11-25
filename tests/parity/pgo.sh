#!/bin/sh
# Times the bench example built three ways against Go: tests/parity/pgo.sh [N]. Needs `oracle` (Go)
# and `bench` (the default build), `bench-pgo` (cargo-pgo, trained on the bench streams) and, when
# BOLT was available, `bench-bolt` (PGO then BOLT) on PATH, and the conformance streams in
# /conformance; run from the repo root (stage `pgo` of .github/ci/Dockerfile, workflow pgo.yml).
# Each build runs tests/parity/bench.sh (its Rust binary is $BENCH, no v4 pass); the tables give
# the Rust ms per MB of each input class, single-threaded and then default threads, and the change
# against the default build.
set -eu
export LC_ALL=C
n=${1:-1000}
t=$(mktemp -d)
have=
labels=
for v in bench:default bench-pgo:PGO bench-bolt:"PGO + BOLT"; do
  b=${v%%:*}
  if ! command -v "$b" >/dev/null 2>&1; then
    echo >&2 "no $b: skipped"
    continue
  fi
  BENCH=$b BENCH_NO_V4=1 sh tests/parity/bench.sh "$n" >"$t/$b.md"
  have="$have $t/$b.md"
  labels="$labels,${v#*:}"
done
[ -n "$have" ] || { echo >&2 "no bench binary on PATH"; exit 1; }
# bench.sh prints the default-threads table, then the one-thread table; a class row is
# `| class | files | MB out | Go ms/MB | Rust ms/MB | Rust / Go |`.
# shellcheck disable=SC2086
awk -F'|' -v labels="${labels#,}" '
FNR == 1 { nf++; tab = 0 }
/^#### / { tab++ }
/^[|] [A-Za-z0-9_]+ [|] [0-9]/ {
  name = $2; gsub(/ /, "", name)
  p = tab == 2 ? 1 : 2
  v[nf, p, name] = $6 + 0
  if (!(name in seen)) { seen[name] = 1; names[++cnt] = name }
}
END {
  split(labels, lab, ",")
  split("single-threaded,default threads", pass, ",")
  for (p = 1; p <= 2; p++) {
    print "#### Rust ms per MB, " pass[p] " (lower is better; the change is against the default build)"
    print ""
    s = "| input class |"; r = "|---|"
    for (f = 1; f <= nf; f++) { s = s " " lab[f] " |"; r = r "---|"; if (f > 1) { s = s " change |"; r = r "---|" } }
    print s; print r
    for (i = 1; i <= cnt; i++) {
      k = names[i]
      s = "| " k " |"
      for (f = 1; f <= nf; f++) {
        s = s sprintf(" %.3f |", v[f, p, k])
        if (f > 1 && v[1, p, k] > 0) s = s sprintf(" %+.1f%% |", (v[f, p, k] / v[1, p, k] - 1) * 100)
      }
      print s
    }
    print ""
  }
}' $have
for f in $have; do
  echo "<details><summary>$(basename "$f" .md): the full bench.sh tables</summary>"
  echo
  cat "$f"
  echo
  echo "</details>"
done
rm -rf "$t"
