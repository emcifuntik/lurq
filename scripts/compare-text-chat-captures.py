"""Compare the captures of two runs of examples/text_chat_gpu_probe.rs.

    python scripts/compare-text-chat-captures.py before/captures.csv after/captures.csv

Every capture of every backend must have the same size and the same pixel
hash in both runs: a change to the text pipeline that is not meant to change
what is drawn leaves all of them equal. Exits with 1 and names the captures
that differ otherwise; their PNGs are next to each `captures.csv`.
"""

import argparse
import csv
import sys
from pathlib import Path


def captures(path):
    with path.open(newline="", encoding="utf-8-sig") as source:
        return {(row["backend"], row["capture"]): (row["width"], row["height"], row["fnv1a"]) for row in csv.DictReader(source)}


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("before", type=Path)
    parser.add_argument("after", type=Path)
    args = parser.parse_args()
    before, after = captures(args.before), captures(args.after)
    if not before:
        parser.error(f"no captures in {args.before}")
    different = [key for key in sorted(before.keys() | after.keys()) if before.get(key) != after.get(key)]
    for backend, capture in different:
        print(f"{backend} {capture}: {before.get((backend, capture))} -> {after.get((backend, capture))}")
    if different:
        print(f"{len(different)} of {len(before)} captures differ")
        sys.exit(1)
    print(f"all {len(before)} captures are pixel-identical")


if __name__ == "__main__":
    main()
