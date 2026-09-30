#!/usr/bin/env python3
"""Aggregate sanpham3 physical-court evidence without upgrading provenance."""

from __future__ import annotations

import argparse
import json
import math
import statistics
import sys
import tempfile
from collections import defaultdict
from pathlib import Path
from typing import Iterable

METRIC_TOKENS = (
    "rtt",
    "bps",
    "latency",
    "startup",
    "join",
    "elapsed",
    "wait",
    "serve",
    "transceive",
    "payload_bytes",
    "rounds",
    "useful_bytes",
)


def parse_evidence(path: Path) -> dict[str, str]:
    fields: dict[str, str] = {}
    for raw in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = raw.strip()
        if line == "--- transcript ---":
            break
        if not line or "=" not in line:
            continue
        key, value = line.split("=", 1)
        key = key.strip()
        value = value.strip()
        if key:
            fields[key] = value
    fields["_path"] = str(path)
    return fields


def evidence_files(paths: Iterable[str]) -> list[Path]:
    found: set[Path] = set()
    for raw in paths:
        path = Path(raw)
        if path.is_file() and path.suffix.lower() == ".txt":
            found.add(path)
        elif path.is_dir():
            found.update(
                item
                for item in path.rglob("*.txt")
                if item.is_file()
            )
    return sorted(found)


def numeric_value(value: str) -> float | None:
    try:
        parsed = float(value)
    except ValueError:
        return None
    return parsed if math.isfinite(parsed) else None


def metric_key(key: str) -> bool:
    lowered = key.lower()
    if lowered in {
        "android_sdk",
        "local_node_id",
        "authenticated_peer_node",
    }:
        return False
    return any(token in lowered for token in METRIC_TOKENS)


def nearest_rank(values: list[float], fraction: float) -> float:
    if not values:
        raise ValueError("nearest_rank requires values")
    if not (0.0 < fraction <= 1.0):
        raise ValueError("fraction must be in (0, 1]")
    ordered = sorted(values)
    rank = max(1, min(len(ordered), math.ceil(fraction * len(ordered))))
    return ordered[rank - 1]


def summarize(records: list[dict[str, str]]) -> dict:
    grouped: dict[tuple[str, str], list[dict[str, str]]] = defaultdict(list)
    skipped: list[str] = []

    for record in records:
        carrier = record.get("carrier")
        role = record.get("role")
        result = record.get("result")
        if not carrier or not role or result not in {"PASS", "FAIL"}:
            skipped.append(record.get("_path", "<unknown>"))
            continue
        grouped[(carrier, role)].append(record)

    groups = []
    for (carrier, role), items in sorted(grouped.items()):
        passed = [item for item in items if item["result"] == "PASS"]
        failed = [item for item in items if item["result"] == "FAIL"]

        metric_values: dict[str, list[float]] = defaultdict(list)
        for item in passed:
            for key, value in item.items():
                if not metric_key(key):
                    continue
                parsed = numeric_value(value)
                if parsed is not None:
                    metric_values[key].append(parsed)

        metrics = {}
        for key, values in sorted(metric_values.items()):
            metrics[key] = {
                "samples": len(values),
                "min": min(values),
                "median": statistics.median(values),
                "p95_nearest_rank": nearest_rank(values, 0.95),
                "max": max(values),
            }

        total = len(items)
        groups.append(
            {
                "carrier": carrier,
                "role": role,
                "records": total,
                "pass": len(passed),
                "fail": len(failed),
                "success_rate": len(passed) / total if total else 0.0,
                "metrics": metrics,
            }
        )

    return {
        "groups": groups,
        "skipped_files": sorted(skipped),
        "provenance_note": (
            "Aggregation does not prove that any input came from a physical "
            "device. Preserve the original court provenance and evidence."
        ),
    }


def render_text(summary: dict) -> str:
    lines = [summary["provenance_note"]]
    if not summary["groups"]:
        lines.append("No valid carrier/role PASS-or-FAIL evidence records found.")
    for group in summary["groups"]:
        lines.append("")
        lines.append(
            f"{group['carrier']} / {group['role']}: "
            f"records={group['records']} pass={group['pass']} "
            f"fail={group['fail']} "
            f"success_rate={group['success_rate']:.4f}"
        )
        for key, stats in group["metrics"].items():
            lines.append(
                f"  {key}: n={stats['samples']} "
                f"min={stats['min']:.6g} "
                f"median={stats['median']:.6g} "
                f"p95={stats['p95_nearest_rank']:.6g} "
                f"max={stats['max']:.6g}"
            )
    if summary["skipped_files"]:
        lines.append("")
        lines.append(
            "Skipped files lacking carrier/role/result: "
            + ", ".join(summary["skipped_files"])
        )
    return "\n".join(lines)


def self_test() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)
        samples = {
            "a.txt": """
timestamp_utc=2026-10-01T00:00:00Z
carrier=rfcomm
role=client
benchmark_rtt_median_ns=100
benchmark_rtt_p95_ns=180
benchmark_one_way_useful_bps=8000
result=PASS

--- transcript ---
ignored=1
""",
            "b.txt": """
carrier=rfcomm
role=client
benchmark_rtt_median_ns=120
benchmark_rtt_p95_ns=200
benchmark_one_way_useful_bps=9000
result=PASS
""",
            "c.txt": """
carrier=rfcomm
role=client
result=FAIL
error=timeout
""",
            "d.txt": """
carrier=local_only_hotspot
role=server
hotspot_startup_ms=450
result=PASS
""",
            "invalid.txt": """
timestamp_utc=2026-10-01T00:00:00Z
result=PASS
""",
        }
        for name, body in samples.items():
            (root / name).write_text(body.strip() + "\n", encoding="utf-8")

        records = [parse_evidence(path) for path in evidence_files([str(root)])]
        result = summarize(records)

        assert len(result["groups"]) == 2
        rfcomm = next(
            group
            for group in result["groups"]
            if group["carrier"] == "rfcomm"
        )
        assert rfcomm["records"] == 3
        assert rfcomm["pass"] == 2
        assert rfcomm["fail"] == 1
        assert abs(rfcomm["success_rate"] - (2 / 3)) < 1e-12
        assert (
            rfcomm["metrics"]["benchmark_rtt_median_ns"]["median"]
            == 110
        )
        assert (
            rfcomm["metrics"]["benchmark_rtt_p95_ns"]["p95_nearest_rank"]
            == 200
        )
        assert len(result["skipped_files"]) == 1

    print("PHYSICAL_EVIDENCE_AGGREGATOR_SELF_TEST_PASS")


def main() -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Aggregate sanpham3 key=value physical-court evidence. "
            "This tool does not upgrade or infer provenance."
        )
    )
    parser.add_argument(
        "paths",
        nargs="*",
        help="Evidence .txt files or directories to scan recursively.",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Emit JSON instead of the human-readable summary.",
    )
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="Run deterministic parser/statistics self-test and exit.",
    )
    args = parser.parse_args()

    if args.self_test:
        self_test()
        return 0

    if not args.paths:
        parser.error("provide at least one evidence file/directory")

    files = evidence_files(args.paths)
    records = [parse_evidence(path) for path in files]
    result = summarize(records)

    if args.json:
        print(json.dumps(result, indent=2, sort_keys=True))
    else:
        print(render_text(result))

    return 0 if result["groups"] else 1


if __name__ == "__main__":
    sys.exit(main())
