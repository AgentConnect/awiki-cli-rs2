#!/usr/bin/env python3
"""Validate Rust coverage thresholds from an lcov report."""

from __future__ import annotations

import argparse
import pathlib
import sys

TOTAL_LABEL = "total"
THRESHOLDS = {
    TOTAL_LABEL: 40.0,
    "crates/awiki-cli/src/app": 30.0,
    "crates/awiki-cli/src/message": 35.0,
    "crates/awiki-cli/src/identity": 50.0,
    "crates/awiki-cli/src/store": 60.0,
    "crates/awiki-cli/src/doctor": 70.0,
    "crates/awiki-cli/src/docs": 100.0,
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Check package and total coverage thresholds from an lcov file."
    )
    parser.add_argument("coverprofile", help="Path to the lcov coverage file.")
    return parser.parse_args()


def normalize_path(raw_path: str) -> str:
    path = pathlib.Path(raw_path)
    try:
        return path.resolve().relative_to(pathlib.Path.cwd().resolve()).as_posix()
    except ValueError:
        text = path.as_posix()
        marker = "crates/awiki-cli/src/"
        index = text.find(marker)
        if index >= 0:
            return text[index:]
        return text


def package_for_file(file_name: str) -> str:
    parts = pathlib.PurePosixPath(file_name).parts
    if len(parts) >= 5 and parts[:4] == ("crates", "awiki-cli", "src", "app"):
        return "crates/awiki-cli/src/app"
    if len(parts) >= 5 and parts[:4] == ("crates", "awiki-cli", "src", "message"):
        return "crates/awiki-cli/src/message"
    if len(parts) >= 5 and parts[:4] == ("crates", "awiki-cli", "src", "identity"):
        return "crates/awiki-cli/src/identity"
    if len(parts) >= 5 and parts[:4] == ("crates", "awiki-cli", "src", "store"):
        return "crates/awiki-cli/src/store"
    if len(parts) >= 5 and parts[:4] == ("crates", "awiki-cli", "src", "doctor"):
        return "crates/awiki-cli/src/doctor"
    if len(parts) >= 5 and parts[:4] == ("crates", "awiki-cli", "src", "docs"):
        return "crates/awiki-cli/src/docs"
    if len(parts) >= 4 and parts[:3] == ("crates", "awiki-cli", "src"):
        return "crates/awiki-cli/src"
    return file_name.rsplit("/", 1)[0] if "/" in file_name else file_name


def load_coverage(path: pathlib.Path) -> tuple[dict[str, tuple[int, int]], tuple[int, int]]:
    package_totals: dict[str, list[int]] = {}
    current_file = ""
    covered_total = 0
    line_total = 0

    with path.open("r", encoding="utf-8") as handle:
        for raw_line in handle:
            line = raw_line.strip()
            if not line:
                continue
            if line.startswith("SF:"):
                current_file = normalize_path(line[3:])
                continue
            if not line.startswith("DA:") or not current_file:
                continue

            _, payload = line.split(":", 1)
            try:
                _, count_text = payload.split(",", 1)
                count = int(count_text.split(",", 1)[0])
            except ValueError as exc:
                raise ValueError(f"invalid lcov DA line: {line}") from exc

            covered = 1 if count > 0 else 0
            package_name = package_for_file(current_file)
            stats = package_totals.setdefault(package_name, [0, 0])
            stats[0] += covered
            stats[1] += 1
            covered_total += covered
            line_total += 1

    frozen_packages = {name: (stats[0], stats[1]) for name, stats in package_totals.items()}
    return frozen_packages, (covered_total, line_total)


def to_percent(covered: int, total: int) -> float:
    if total <= 0:
        return 0.0
    return (covered / total) * 100.0


def main() -> int:
    args = parse_args()
    coverprofile = pathlib.Path(args.coverprofile)
    if not coverprofile.is_file():
        print(f"coverage file not found: {coverprofile}", file=sys.stderr)
        return 1

    package_totals, total_stats = load_coverage(coverprofile)
    failures: list[tuple[str, float, float]] = []

    total_percent = to_percent(*total_stats)
    if total_percent < THRESHOLDS[TOTAL_LABEL]:
        failures.append((TOTAL_LABEL, total_percent, THRESHOLDS[TOTAL_LABEL]))

    for package_name, threshold in THRESHOLDS.items():
        if package_name == TOTAL_LABEL:
            continue
        percent = to_percent(*package_totals.get(package_name, (0, 0)))
        if percent < threshold:
            failures.append((package_name, percent, threshold))

    print("Coverage summary:")
    print(f"  {TOTAL_LABEL:32} {total_percent:6.1f}% (threshold {THRESHOLDS[TOTAL_LABEL]:.1f}%)")
    for package_name in sorted(name for name in THRESHOLDS if name != TOTAL_LABEL):
        percent = to_percent(*package_totals.get(package_name, (0, 0)))
        print(f"  {package_name:32} {percent:6.1f}% (threshold {THRESHOLDS[package_name]:.1f}%)")

    if failures:
        print("Coverage check failed:", file=sys.stderr)
        for package_name, percent, threshold in failures:
            print(
                f"  {package_name}: {percent:.1f}% < required {threshold:.1f}%",
                file=sys.stderr,
            )
        return 1

    print("Coverage check passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
