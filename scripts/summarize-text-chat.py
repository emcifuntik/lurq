"""Summarize the chat text benchmarks using only the Python standard library.

    python scripts/summarize-text-chat.py metrics.csv [more.csv ...]
    python scripts/summarize-text-chat.py --criterion run1.txt run2.txt run3.txt

CSV mode reads the per-frame rows of `benches/text_chat/metrics.rs` or of
`examples/text_chat_gpu_probe.rs`. A run is one `run` value of one file (the
probe has no `run` column: each of its files is a run). Each cell is the
median over runs of the run's median over frames, with the lowest and the
highest run after it.

Criterion mode reads saved `cargo bench --bench text_chat` outputs, one run
per file, and gives the median of their point estimates the same way.
"""

import argparse
import csv
import re
import statistics
from collections import defaultdict
from pathlib import Path

KEYS = ("backend", "scenario")
SKIPPED = {"run", "frame"}
TIME = re.compile(r"time:\s+\[\S+ \S+ (\S+) (\S+) \S+ \S+\]")
UNITS = {"ns": 0.001, "µs": 1.0, "us": 1.0, "ms": 1000.0, "s": 1_000_000.0}


def spread(values):
    return statistics.median(values), min(values), max(values)


def cell(values, digits):
    if not values:
        return "-"
    median, low, high = spread(values)
    return f"{median:.{digits}f} [{low:.{digits}f}..{high:.{digits}f}]"


def csv_runs(paths):
    """(key, column) -> one median per run."""
    frames = defaultdict(list)
    columns = []
    for path in paths:
        with path.open(newline="", encoding="utf-8-sig") as source:
            reader = csv.DictReader(source)
            for column in reader.fieldnames:
                if column not in KEYS and column not in SKIPPED and column not in columns:
                    columns.append(column)
            for row in reader:
                key = tuple(row[name] for name in KEYS if name in row)
                run = (str(path), row.get("run", ""))
                for column in columns:
                    if row.get(column, "") != "":
                        frames[key, column, run].append(float(row[column]))
    runs = defaultdict(list)
    for (key, column, _run), values in frames.items():
        runs[key, column].append(statistics.median(values))
    keys = list(dict.fromkeys(key for key, _column in runs))
    return keys, columns, runs


def criterion_runs(paths):
    """benchmark -> one point estimate in microseconds per run."""
    runs = defaultdict(list)
    for path in paths:
        name = None
        for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
            line = line.strip()
            match = TIME.search(line)
            if match and (name or not line.startswith("time:")):
                name = name or line.split()[0]
                runs[name].append(float(match.group(1)) * UNITS[match.group(2)])
                name = None
            elif line.startswith("text_chat/") or line.startswith("text_pipeline/"):
                name = line.split()[0]
    return runs


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("files", type=Path, nargs="+")
    parser.add_argument("--criterion", action="store_true", help="the files are saved cargo bench outputs")
    parser.add_argument("--columns", help="comma-separated columns to print (CSV mode); all by default")
    parser.add_argument("--csv", type=Path, help="also save every column's median, lowest and highest run here")
    args = parser.parse_args()

    if args.criterion:
        runs = criterion_runs(args.files)
        if not runs:
            parser.error("no Criterion estimates in the given files")
        width = max(len(name) for name in runs)
        print(f"{'benchmark':<{width}}  runs  median [lowest..highest], microseconds")
        for name, values in runs.items():
            print(f"{name:<{width}}  {len(values):>4}  {cell(values, 1)}")
        return

    keys, columns, runs = csv_runs(args.files)
    if not keys:
        parser.error("no rows in the given files")
    if args.csv:
        with args.csv.open("w", newline="", encoding="utf-8") as dest:
            writer = csv.writer(dest)
            writer.writerow(["key", "column", "runs", "median", "lowest", "highest"])
            for key in keys:
                for column in columns:
                    values = runs.get((key, column))
                    if values:
                        writer.writerow([" ".join(key), column, len(values), *spread(values)])
    shown = args.columns.split(",") if args.columns else columns
    unknown = [column for column in shown if column not in columns]
    if unknown:
        parser.error(f"unknown columns: {', '.join(unknown)}")
    width = max(len(" ".join(key)) for key in keys)
    for column in shown:
        digits = 1 if column.endswith("_us") else 0
        print(f"\n{column}: median over runs of the run's median frame [lowest run..highest run]")
        for key in keys:
            values = runs.get((key, column), [])
            print(f"  {' '.join(key):<{width}}  {cell(values, digits)}")


if __name__ == "__main__":
    main()
