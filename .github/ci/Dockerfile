# CI image: every check runs in this Dockerfile under BuildKit, so CI and a local `docker buildx build` run the same steps.
# The one dependency (rayon) is fetched in the toolchain stage, before the source is copied, and
# every later stage builds offline from that layer (so the cargo registry is not a cache mount: it
# would hide it).
# Every tool is installed in a stage before `COPY . .`, so a new commit does not install it again.
FROM rust:1.99.1-bookworm AS toolchain
RUN rustup component add rustfmt clippy && rustup target add aarch64-unknown-linux-gnu
WORKDIR /src
# The manifests and lockfiles of the crate and of the fuzz crate, with stub targets, so the fetch is
# a layer that only a change to them rebuilds.
COPY Cargo.toml Cargo.lock ./
COPY tests/fuzz/Cargo.toml tests/fuzz/Cargo.lock tests/fuzz/
RUN mkdir -p src tests/fuzz/fuzz_targets && touch src/lib.rs \
    && for t in $(sed -n 's#^path = "fuzz_targets/\(.*\)\.rs"$#\1#p' tests/fuzz/Cargo.toml); do echo 'fn main() {}' > tests/fuzz/fuzz_targets/$t.rs; done \
    && cargo fetch --locked && cargo fetch --locked --manifest-path tests/fuzz/Cargo.toml
ENV CARGO_NET_OFFLINE=true

FROM toolchain AS src
COPY . .

# The ITU-T T.834 conformance streams, as Go's testdata/conformance/fetch.sh fetches them: ITU's WAF
# sometimes serves a wrong body, so retry until the sha256 matches. A layer of its own, so BuildKit
# caches it across builds: once a download with the right sha256 succeeds, the layer (the
# extracted t834.zip) is in the Actions cache of each job and ITU is not asked again.
FROM rust:1.99.1-bookworm AS conformance
RUN apt-get update && apt-get install -y --no-install-recommends unzip && rm -rf /var/lib/apt/lists/*
# ITU's WAF answers a plain curl with "Request Rejected"; the browser-like headers below get through.
# Hosted runner IPs can still be blocked. Then the stage fails (so no empty corpus is cached), unless
# the build arg ALLOW_NO_CONFORMANCE=1 is set: then it warns, the corpus is empty, and the tests and
# the oracle skip it, as they do on a checkout without it (the conformance coverage is lost).
ARG ALLOW_NO_CONFORMANCE=0
RUN mkdir -p /corpus/conformance/software/JXR_ConformanceSuite; \
    for i in 1 2 3 4 5 6 7 8; do \
      curl -sSL -A "Mozilla/5.0 (X11; Linux x86_64; rv:128.0) Gecko/20100101 Firefox/128.0" \
        -H "Accept: text/html,application/xhtml+xml,application/zip,*/*;q=0.8" -H "Accept-Language: en-US,en;q=0.5" \
        -e "https://www.itu.int/rec/T-REC-T.834-201001-S/en" \
        -w 'attempt '$i': http %{http_code} %{size_download} bytes\n' -o /t834.zip \
        "https://www.itu.int/rec/dologin_pub.asp?lang=e&id=T-REC-T.834-201001-S!!SOFT-ZST-E&type=items" || true; \
      [ "$(sha256sum /t834.zip | cut -d' ' -f1)" = 808fdd13ebd91f5111449af195e6b885d919163d37fcc7ac63c259893f17fb52 ] && break; \
      sleep 5; \
    done; \
    if [ "$(sha256sum /t834.zip | cut -d' ' -f1)" = 808fdd13ebd91f5111449af195e6b885d919163d37fcc7ac63c259893f17fb52 ]; then \
      unzip -oq /t834.zip -d /corpus/conformance; \
    elif [ "$ALLOW_NO_CONFORMANCE" = 1 ]; then \
      echo "WARNING: conformance corpus NOT fetched (ITU rejected the request); its tests and parity inputs are skipped"; \
    else \
      echo "conformance corpus NOT fetched (ITU rejected the request); set the build arg ALLOW_NO_CONFORMANCE=1 to skip it"; \
      exit 1; \
    fi

# The checks run in the build, so a red check fails the build; a rerun of the same source is a
# BuildKit cache hit. The target dir is a cache mount, so it persists across builds.
FROM src AS ci
# Gates A and B, the pinned hashes and decode_component run on the conformance streams (they skip
# them when corpus/ is absent).
COPY --from=conformance /corpus corpus
RUN --mount=type=cache,id=jpegxr-target,target=/src/target \
    cargo fmt --check
RUN --mount=type=cache,id=jpegxr-target,target=/src/target \
    cargo clippy --locked --all-targets -- -D warnings
# The NEON module is not compiled for x86_64, so lint the aarch64 target too.
RUN --mount=type=cache,id=jpegxr-target,target=/src/target \
    cargo clippy --locked --all-targets --target aarch64-unknown-linux-gnu -- -D warnings
RUN --mount=type=cache,id=jpegxr-target,target=/src/target \
    cargo test --locked
RUN --mount=type=cache,id=jpegxr-target,target=/src/target \
    JPEGXR_SIMD=scalar cargo test --locked
RUN --mount=type=cache,id=jpegxr-target,target=/src/target \
    cargo build --locked --release --target aarch64-unknown-linux-gnu
# The x86-64-v4 build (docs/performance.md, the bench's v4 column) must pass the same tests: the
# unit tests and the pinned hashes of tests/jpegxr.rs, released, with its own cache mount since
# RUSTFLAGS changes every artifact. The conformance corpus is copied above.
RUN --mount=type=cache,id=jpegxr-target-v4,target=/src/target \
    if grep -qw avx512bw /proc/cpuinfo; then \
      RUSTFLAGS="-C target-cpu=x86-64-v4 -C target-feature=+false-deps-tzcnt,+false-deps-bls" cargo test --locked --release --lib --test jpegxr; \
    else echo "no AVX-512 on this runner: x86-64-v4 tests skipped"; fi

# aarch64 tests on a native arm64 runner, both SIMD tiers (NEON, then JPEGXR_SIMD=scalar): only the
# tests that the target changes. The unit tests (--lib) hold the NEON kernel tests, the
# tier-against-scalar tests and the tier detection in src/codec/simd.rs; tests/jpegxr.rs decodes the
# streams through the NEON tiers and checks the pinned hashes. tests/alloc.rs does not touch SIMD.
# The full suite runs on x86_64 in `ci`; Go parity on aarch64 is the parity-arm64 stage.
FROM src AS arm64
RUN --mount=type=cache,id=jpegxr-target,target=/src/target \
    cargo test --locked --lib --test jpegxr
RUN --mount=type=cache,id=jpegxr-target,target=/src/target \
    JPEGXR_SIMD=scalar cargo test --locked --lib --test jpegxr

# Go parity: the Go oracle writes the inputs and decodes them; the Rust example decodes the same
# inputs; the two output trees must be equal.
FROM golang:1.27-bookworm AS go-oracle
ENV GOTOOLCHAIN=local CGO_ENABLED=0
WORKDIR /src
COPY tests/parity/go tests/parity/go
# kernelgen is inside the upstream module (it imports internal/codec), see docs/parity.md.
RUN --mount=type=cache,id=jpegxr-go-build,target=/root/.cache/go-build cd tests/parity/go/oracle && go build -o /out/oracle . \
    && cd ../upstream && go build -o /out/kernelgen ./cmd/kernelgen
# The same oracle built for x86-64-v4 (the bench's v4 column); the parity stages use /out/oracle.
RUN --mount=type=cache,id=jpegxr-go-build-v4,target=/root/.cache/go-build cd tests/parity/go/oracle && GOAMD64=v4 go build -o /out/oracle-v4 .

FROM go-oracle AS go-run
COPY tests/data tests/data
COPY --from=conformance /corpus/conformance/software/JXR_ConformanceSuite /conformance
# Inputs: the tests/data streams and their mutations, and the conformance streams (see docs/parity.md).
RUN /out/oracle -gen tests/data /inputs /conformance && ls /inputs | wc -l && du -sh /inputs
RUN /out/oracle /inputs /go-out && ls /go-out | wc -l && du -sh /go-out
# The inputs of the six random-input kernel tests and the generic Go kernels' digests.
RUN /out/kernelgen /kernels /go-kernels && du -sh /kernels /go-kernels
# The subset the aarch64 oracle runs (stage parity-arm64): every input that is not a mutation (`~`
# in its name) or a conformance stream, and every 20th mutation. Go decodes it again, so the two
# trees have the same names.
RUN mkdir /sub && ls /inputs | awk '(!/~/ && !/^conf-/) || NR % 20 == 0' | (cd /inputs && xargs cp -t /sub) \
    && /out/oracle /sub /go-sub && ls /sub | wc -l && ls /go-sub | wc -l
# The unmutated streams again under small memory limits (`ORACLE_MEM`, bytes), where the estimate
# decides between success and class alloc: Go and Rust must agree on which side of each limit a
# stream falls.
RUN mkdir -p /base /go-mem && ls /inputs | grep -v '~' | (cd /inputs && xargs cp -t /base) \
    && for n in 65536 80000 96000 112000 128000 160000 192000 256000 384000 512000 768000 1048576; do \
         ORACLE_MEM=$n /out/oracle /base /go-mem/$n || exit 1; done

# The dev profile step runs the aarch64 subset: its overflow checks are on (what a consumer's debug build
# has: Cargo ignores this crate's [profile] tables for a dependent). A wrap-around the code does not
# spell out panics, and the oracle would print `internal` where Go prints pixels. Go printing
# `internal` is a failure too (Go's FuzzDecode rule).
FROM src AS parity
# The Go trees are bind mounts from go-run, not COPY: a COPY of them after `COPY . .` would run on
# every commit. Each Rust output tree is deleted right after its diff (the 10 GB of one run filled
# a disk once), so the layer (and the cache export) stays empty.
ARG ALLOW_NO_CONFORMANCE=0
RUN --mount=type=cache,id=jpegxr-target,target=/src/target \
    --mount=type=bind,from=go-run,source=/inputs,target=/inputs \
    --mount=type=bind,from=go-run,source=/go-out,target=/go-out \
    --mount=type=bind,from=go-run,source=/base,target=/base \
    --mount=type=bind,from=go-run,source=/sub,target=/sub \
    --mount=type=bind,from=go-run,source=/go-sub,target=/go-sub \
    --mount=type=bind,from=go-run,source=/go-mem,target=/go-mem \
    --mount=type=bind,from=go-run,source=/kernels,target=/kernels \
    --mount=type=bind,from=go-run,source=/go-kernels,target=/go-kernels \
    cargo run --locked --release --example oracle -- -kernels /kernels /rust-kernels \
    && { bad=0; for t in scalar simd; do for f in $(ls /go-kernels); do \
         cmp /go-kernels/$f /rust-kernels/$t/$f || { echo "kernel $f, tier $t: case = (byte - 1) / 8"; bad=1; }; done; done; [ $bad = 0 ]; } \
    && echo "$(( $(cat /go-kernels/*.out | wc -c) / 8 )) kernel cases, equal for the scalar kernels and the SIMD tier" \
    && rm -rf /rust-kernels \
    && cargo run --locked --release --example oracle -- /inputs /rust-out \
    && echo "$(ls /inputs | wc -l) inputs, $(ls /go-out | wc -l) outputs" \
    && [ "$(ls /go-out | wc -l)" -ge 100000 ] \
    && { [ "$ALLOW_NO_CONFORMANCE" = 1 ] || [ "$(ls /inputs | grep -c '^conf-')" -ge 400 ] || { echo "conformance streams missing from the parity inputs (set ALLOW_NO_CONFORMANCE=1 to skip)"; exit 1; }; } \
    && { diff -rq /go-out /rust-out >/tmp/diff.txt || { head -50 /tmp/diff.txt; wc -l /tmp/diff.txt; exit 1; }; } \
    && rm -rf /rust-out \
    && JPEGXR_SIMD=scalar cargo run --locked --release --example oracle -- /inputs /rust-out-scalar \
    && { diff -rq /go-out /rust-out-scalar >/tmp/diff.txt || { head -50 /tmp/diff.txt; wc -l /tmp/diff.txt; exit 1; }; } \
    && rm -rf /rust-out-scalar \
    && { ! grep -rlx internal /go-out | head -20 | grep . ; } \
    && for n in $(ls /go-mem); do \
         ORACLE_MEM=$n cargo run --locked --release --example oracle -- /base /rust-mem-$n \
         && { diff -rq /go-mem/$n /rust-mem-$n >/tmp/diff.txt || { echo "ORACLE_MEM=$n"; head -50 /tmp/diff.txt; wc -l /tmp/diff.txt; exit 1; }; } \
         && rm -rf /rust-mem-$n || exit 1; done \
    && ORACLE_THREADS=1 cargo run --locked --example oracle -- /sub /rust-out-checked \
    && { diff -rq /go-sub /rust-out-checked >/tmp/diff.txt || { head -50 /tmp/diff.txt; wc -l /tmp/diff.txt; exit 1; }; } \
    && rm -rf /rust-out-checked

# aarch64 against Go: the oracle example run on a native arm64 runner (the NEON tiers), on the
# subset, once per SIMD tier; the output must equal Go's.
FROM src AS parity-arm64
RUN --mount=type=cache,id=jpegxr-target,target=/src/target \
    --mount=type=bind,from=go-run,source=/sub,target=/sub \
    --mount=type=bind,from=go-run,source=/go-sub,target=/go-sub \
    --mount=type=bind,from=go-run,source=/kernels,target=/kernels \
    --mount=type=bind,from=go-run,source=/go-kernels,target=/go-kernels \
    cargo run --locked --release --example oracle -- -kernels /kernels /arm-kernels \
    && { bad=0; for t in scalar simd; do for f in $(ls /go-kernels); do \
         cmp /go-kernels/$f /arm-kernels/$t/$f || { echo "kernel $f, tier $t: case = (byte - 1) / 8"; bad=1; }; done; done; [ $bad = 0 ]; } \
    && echo "$(( $(cat /go-kernels/*.out | wc -c) / 8 )) kernel cases on aarch64, equal for the scalar kernels and NEON" \
    && rm -rf /arm-kernels \
    && cargo run --locked --release --example oracle -- /sub /arm-out \
    && echo "$(ls /sub | wc -l) inputs, $(ls /go-sub | wc -l) outputs" \
    && [ "$(ls /go-sub | wc -l)" -ge 5000 ] \
    && { diff -rq /go-sub /arm-out >/tmp/diff.txt || { head -50 /tmp/diff.txt; wc -l /tmp/diff.txt; exit 1; }; } \
    && rm -rf /arm-out \
    && JPEGXR_SIMD=scalar cargo run --locked --release --example oracle -- /sub /arm-out-scalar \
    && { diff -rq /go-sub /arm-out-scalar >/tmp/diff.txt || { head -50 /tmp/diff.txt; wc -l /tmp/diff.txt; exit 1; }; } \
    && rm -rf /arm-out-scalar

# Fuzzing (fuzz.yml): nightly and cargo-fuzz, with the targets built into the image. The
# workflow runs it with `docker run`, so a BuildKit cache hit cannot skip the fuzzing.
FROM toolchain AS fuzz-tools
RUN rustup toolchain install nightly --profile minimal \
    && CARGO_NET_OFFLINE=false cargo install cargo-fuzz --locked --version 0.13.1

FROM fuzz-tools AS fuzz
COPY . .
RUN cargo +nightly fuzz build --fuzz-dir tests/fuzz

# Benchmark (bench.yml): the Go oracle and the Rust bench example in one image, each built twice:
# the default build, and x86-64-v4 (GOAMD64=v4; RUSTFLAGS as docs/performance.md recommends) as
# oracle-v4 and bench-v4; tests/parity/bench.sh prints both.
FROM src AS bench
COPY --from=go-oracle /out/oracle /usr/local/bin/oracle
COPY --from=go-oracle /out/oracle-v4 /usr/local/bin/oracle-v4
COPY --from=conformance /corpus/conformance/software/JXR_ConformanceSuite /conformance
RUN --mount=type=cache,id=jpegxr-target,target=/src/target \
    cargo build --locked --release --example bench && cp target/release/examples/bench /usr/local/bin/bench
# Its own cache mount (the v4 target dir), as in `ci`.
RUN --mount=type=cache,id=jpegxr-target-v4,target=/src/target \
    RUSTFLAGS="-C target-cpu=x86-64-v4 -C target-feature=+false-deps-tzcnt,+false-deps-bls" cargo build --locked --release --example bench \
    && cp target/release/examples/bench /usr/local/bin/bench-v4

# PGO and BOLT (pgo.yml, dispatch only; docs/performance.md): cargo-pgo builds the bench example
# instrumented, trains it, rebuilds it with the profile, and, when BOLT is installed, once more
# through BOLT. The tools are installed before `COPY . .`, so a new commit does not install them
# again. BOLT comes from apt.llvm.org (bookworm has none of its own); if the repository or the
# package is missing the image still builds and the job reports PGO only.
FROM toolchain AS pgo-tools
RUN (rustup component add llvm-tools || rustup component add llvm-tools-preview) \
    && CARGO_NET_OFFLINE=false cargo install cargo-pgo --locked
RUN apt-get update && apt-get install -y --no-install-recommends wget gnupg \
    && { wget -qO- https://apt.llvm.org/llvm-snapshot.gpg.key | gpg --dearmor -o /usr/share/keyrings/llvm.gpg \
         && echo "deb [signed-by=/usr/share/keyrings/llvm.gpg] http://apt.llvm.org/bookworm/ llvm-toolchain-bookworm-19 main" > /etc/apt/sources.list.d/llvm.list \
         && apt-get update && apt-get install -y --no-install-recommends bolt-19 libbolt-19-dev \
         && ln -s /usr/lib/llvm-19/bin/llvm-bolt /usr/local/bin/llvm-bolt \
         && ln -s /usr/lib/llvm-19/bin/merge-fdata /usr/local/bin/merge-fdata \
         && ln -s "$(find /usr/lib/llvm-19 -name libbolt_rt_instr.a | head -n 1)" /usr/local/lib/libbolt_rt_instr.a \
         && test -s /usr/local/lib/libbolt_rt_instr.a; } \
    || echo "BOLT is not available: the PGO job reports PGO only"; \
    rm -rf /var/lib/apt/lists/*

# bench (default build), bench-pgo and, when BOLT worked, bench-bolt (PGO then BOLT), all in
# /usr/local/bin with the Go oracle; tests/parity/pgo.sh times them. The profiles of an earlier run
# stay in the cache mount, so they are deleted first. The training set (/train) is the bench's own
# streams: tests/data/synth and the same conformance subset bench.sh picks. The PGO profile is
# trained on one thread and on the default threads; the BOLT profile on one thread only.
FROM pgo-tools AS pgo
COPY . .
COPY --from=go-oracle /out/oracle /usr/local/bin/oracle
COPY --from=conformance /corpus/conformance/software/JXR_ConformanceSuite /conformance
RUN mkdir -p /train && cp tests/data/synth/*.jxr /train \
    && find /conformance -iname '*.jxr' -size +30k -size -400k 2>/dev/null | sort | awk 'NR % 6 == 1' | head -30 | \
       while read -r f; do cp "$f" "/train/conf-$(echo "${f#/conformance/}" | tr '/ ' '__')"; done; ls /train | wc -l
RUN --mount=type=cache,id=jpegxr-target-pgo,sharing=locked,target=/src/target \
    find src examples tests -type f -exec touch {} + \
    && rm -rf target/pgo-profiles target/bolt-profiles && find target -name 'bench-bolt-*' -delete \
    && host=$(rustc -vV | sed -n 's/^host: //p') && bin=target/$host/release/examples/bench \
    && cargo build --locked --release --example bench && cp target/release/examples/bench /usr/local/bin/bench \
    && cargo pgo build -- --locked --example bench \
    && BENCH_THREADS=1 cargo pgo run -- --locked --example bench -- /train 100 >/dev/null \
    && cargo pgo run -- --locked --example bench -- /train 100 >/dev/null \
    && cargo pgo optimize build -- --locked --example bench && cp $bin /usr/local/bin/bench-pgo \
    && if command -v llvm-bolt >/dev/null && command -v merge-fdata >/dev/null; then \
         { cargo pgo bolt build --with-pgo -- --locked --example bench \
           && BENCH_THREADS=1 "$(dirname $bin)/bench-bolt-instrumented" /train 100 >/dev/null \
           && cargo pgo bolt optimize --with-pgo -- --locked --example bench \
           && cp "$(find target -type f -name 'bench-bolt-optimized' | head -n 1)" /usr/local/bin/bench-bolt; } \
         || echo "BOLT failed: the job reports PGO only"; \
       else echo "no BOLT: the job reports PGO only"; fi
