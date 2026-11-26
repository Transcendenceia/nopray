# Benchmark plan: NOPRAY vs C, Rust, Go, JavaScript, Python

Status: **executed and closed (2026-11-26)**. This file preserves the original methodology plan.
Completed empirical results (`72/72` execution cells, `70/70` multi-model authoring cells / `9,990`
hidden tests, 12-run forensic purity audit) are summarized in [`BENCHMARKS.md`](./BENCHMARKS.md).

## Questions to answer

1. Runtime footprint: size on disk, RSS, startup latency, syscall count.
2. Throughput and latency where they matter (HTTP server, numeric kernel).
3. Authoring cost: agent time, tokens, iterations until correct, defects found by audit/tests.
4. Modification cost: same change requested on every implementation; time and defects.
5. Auditability: time for an independent reviewer (human or agent) to answer fixed questions
   ("which syscalls can it make?", "can a request write outside buffer X?").
6. Security posture: attack surface (syscalls, dependencies, bytes of code), hardening available.

## Workloads (same specification for every language)

| ID | Program | Why |
|---|---|---|
| W1 | `hello`: print a line, exit 0 | floor of size/startup |
| W2 | `wc`-like: count bytes/lines/words of stdin | I/O loop, parsing |
| W3 | Static HTTP/1.1 server: GET/HEAD, 3 routes, 404/405, path traversal rejected, `sendfile` where available | the nopray-httpd case |
| W4 | Fixed-point calculator core: 10^6 operations from a file | arithmetic and branches |
| W5 | Modification task on W3: add a route and a header | change cost |

## Implementations and build rules

- C: `musl-gcc -static -O2`, stripped; also `-Os`.
- Rust: `--release`, `panic = "abort"`, `opt-level = "s"` variant, static musl target, stripped.
- Go: `CGO_ENABLED=0 go build -ldflags="-s -w"`.
- JavaScript: Node.js current LTS (runtime size reported separately), also Bun if installed.
- Python: CPython current (runtime size reported separately).
- NOPRAY: tier L2 apps mapped with `nopray.toml`, `audit --strict` clean.
- Same functional test suite passes for every implementation before any measurement.

## Measurements and tools

| Metric | Tool |
|---|---|
| Size on disk (binary; runtime separately for JS/Python) | `stat -c %s`, `du` |
| Startup latency | `hyperfine --warmup 20 -N` |
| Peak RSS | `/usr/bin/time -v` |
| Syscalls (count and distinct set) | `strace -f -c`, `strace -f -qq -e trace=all` |
| HTTP throughput / p99 latency | `oha` or `wrk`, fixed connections and duration, loopback |
| CPU kernel | `hyperfine` on W4 |
| Attack surface | distinct syscalls, linked libraries, lines/instructions reviewed |
| Agent cost | wall time, tokens, iterations, audit/test failures (from session logs) |
| Review cost | time and correctness of answers to a fixed question list |

Run on one idle host (HP CachyOS or a dedicated VM), CPU governor fixed, 10 repetitions, report
median and spread, publish raw data and scripts next to the results.

## Fairness rules

- Equal features and equal error handling; no NOPRAY shortcut that the others did not get.
- Report runtime sizes for JavaScript and Python instead of hiding them.
- Include failures: if a NOPRAY version needed more iterations or shipped a defect, it counts.
- The benchmark harness itself follows First Rust (or shell) and is not part of the comparison.
