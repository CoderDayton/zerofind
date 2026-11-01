#!/usr/bin/env python3
"""zf vs fd on one directory, with equivalent semantics.

Every query pair is first checked for an identical match set; timings only count when the
sets agree. Runs alternate zf/fd so drift on a shared machine hits both. The page cache is
warm (no root to drop it), so "cold start" here means process start, not disk.

Wall time and peak RSS come from benches/peakrss.c (built by `cgs run bench`); the first-result
time is taken here and includes the launcher's own start (~1 ms, the same for both tools).

usage: compare.py [--runs N] [--name QUERY] [--zf PATH] [--fd-threads N] DIR
"""

import argparse
import os
import shutil
import statistics
import subprocess
import tempfile
import time

# fd flags that approximate zf's default policy: case-insensitive, git rules inside repos only,
# and zf's generated directories. fd cannot check zf's markers (Cargo.toml beside target/, ...),
# so the match-set check fails on trees with an unmarked target/, build/, dist/, out/,
# coverage/ or vendor/; hidden ones fd skips anyway.
FD_COMMON = ["--color", "never", "-i",
             "-E", "node_modules", "-E", "__pycache__", "-E", "target", "-E", "dist",
             "-E", "build", "-E", "out", "-E", "coverage", "-E", "vendor"]

def queries(name):
    return [
        ("name", [name], ["-F", name]),
        ("ext", [".rs"], ["-e", "rs"]),
        ("dirs", ["src", "--dirs"], ["-F", "-t", "d", "src"]),
    ]


PEAKRSS = os.path.join(os.path.dirname(os.path.abspath(__file__)), "../target/peakrss")


def run_once(cmd, cwd):
    """Wall time, time to first output byte, peak RSS (KB) and stdout of one run."""
    start = time.perf_counter()
    proc = subprocess.Popen([PEAKRSS, *cmd], cwd=cwd,
                            stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    first = None
    chunks = []
    while chunk := proc.stdout.read1(65536):
        if first is None:
            first = time.perf_counter() - start
        chunks.append(chunk)
    report = proc.stderr.read().split()
    # 127: peakrss could not exec the command, so the timing is of nothing.
    if proc.wait() == 127:
        raise SystemExit(f"could not run {cmd[0]}")
    rss_kb, wall_ns = int(report[1]), int(report[2])
    return wall_ns / 1e9, first, rss_kb, b"".join(chunks)


def records(out):
    # fd marks directories with a trailing '/'; zf prints the bare path.
    return {r.rstrip(b"/") for r in out.split(b"\n")} - {b""}


def bench(label, zf_cmd, fd_cmd, cwd, runs):
    _, _, _, zf_out = run_once(zf_cmd, cwd)
    _, _, _, fd_out = run_once(fd_cmd, cwd)
    zs, fs = records(zf_out), records(fd_out)
    if zs != fs:
        only_zf, only_fd = sorted(zs - fs)[:3], sorted(fs - zs)[:3]
        print(f"{label:<6} MISMATCH zf={len(zs)} fd={len(fs)} "
              f"zf-only={only_zf} fd-only={only_fd}")
        return
    samples = {"zf": [], "fd": []}
    for _ in range(runs):
        for tool, cmd in (("zf", zf_cmd), ("fd", fd_cmd)):
            samples[tool].append(run_once(cmd, cwd)[:3])
    row = [f"{label:<6} {len(zs):>8}"]
    for tool in ("zf", "fd"):
        walls = [s[0] for s in samples[tool]]
        firsts = [s[1] for s in samples[tool] if s[1] is not None]
        rss = max(s[2] for s in samples[tool])
        ttfr = f"{statistics.median(firsts) * 1e3:7.1f}" if firsts else "      -"
        row.append(f"{statistics.median(walls) * 1e3:8.1f} {ttfr} {rss / 1024:6.1f}")
    print("  ".join(row))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("dir")
    ap.add_argument("--runs", type=int, default=10)
    ap.add_argument("--name", default="main", help="query for the name row")
    ap.add_argument("--zf", default=os.path.join(os.path.dirname(__file__), "../target/release/zf"))
    ap.add_argument("--fd-threads", type=int, default=0, help="0 = fd default")
    args = ap.parse_args()
    zf = os.path.abspath(args.zf)
    fd = shutil.which("fd")
    fd_extra = ["-j", str(args.fd_threads)] if args.fd_threads else []

    print(f"{args.dir}  (median of {args.runs}; ms, ms to first result, peak MB)")
    print(f"{'query':<6} {'matches':>8}  {'zf wall':>8} {'first':>7} {'rss':>6}  "
          f"{'fd wall':>8} {'first':>7} {'rss':>6}")
    with tempfile.TemporaryDirectory() as empty:
        bench("true", ["true"], ["true"], empty, args.runs)
        bench("start", [zf, "x"], [fd, *FD_COMMON, *fd_extra, "-F", "x"], empty, args.runs * 5)
    for label, zq, fq in queries(args.name):
        bench(label, [zf, *zq], [fd, *FD_COMMON, *fd_extra, *fq], args.dir, args.runs)


if __name__ == "__main__":
    main()
