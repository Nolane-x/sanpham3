#!/usr/bin/env python3
"""Write validated operator observations into a sanpham3 evidence campaign."""

from __future__ import annotations

import argparse
import math
import subprocess
from datetime import datetime, timezone
from pathlib import Path

CARRIERS = {
    "nfc_hce",
    "ble_gatt",
    "rfcomm",
    "local_only_hotspot",
}


def positive_float(name: str, raw: str) -> float:
    try:
        value = float(raw)
    except ValueError as error:
        raise argparse.ArgumentTypeError(
            f"{name} must be numeric"
        ) from error
    if not math.isfinite(value) or value <= 0.0:
        raise argparse.ArgumentTypeError(
            f"{name} must be finite and > 0"
        )
    return value


def nonnegative_float(name: str, raw: str) -> float:
    try:
        value = float(raw)
    except ValueError as error:
        raise argparse.ArgumentTypeError(
            f"{name} must be numeric"
        ) from error
    if not math.isfinite(value) or value < 0.0:
        raise argparse.ArgumentTypeError(
            f"{name} must be finite and >= 0"
        )
    return value


def git_commit() -> str:
    try:
        return subprocess.check_output(
            ["git", "rev-parse", "HEAD"],
            text=True,
            stderr=subprocess.DEVNULL,
        ).strip()
    except (OSError, subprocess.CalledProcessError):
        return "unknown"


def safe_slug(value: str) -> str:
    cleaned = "".join(
        ch if ch.isalnum() or ch in "._-" else "_"
        for ch in value
    )
    return cleaned.strip("._-") or "observation"


def parse_bool(raw: str) -> bool:
    lowered = raw.strip().lower()
    if lowered == "true":
        return True
    if lowered == "false":
        return False
    raise argparse.ArgumentTypeError(
        "boolean value must be true or false"
    )


def write_observation(args: argparse.Namespace) -> Path:
    out = Path(args.evidence_dir)
    out.mkdir(parents=True, exist_ok=True)

    if args.energy_joules is None and args.energy_method is not None:
        raise ValueError(
            "--energy-method requires --energy-joules"
        )
    if args.energy_joules is not None and not args.energy_method:
        raise ValueError(
            "--energy-joules requires --energy-method"
        )

    supplied = [
        args.setup_latency_ms is not None,
        args.range_m is not None,
        args.energy_joules is not None,
        args.concurrent_internet is not None,
    ]
    if not any(supplied):
        raise ValueError(
            "provide at least one physical observation metric"
        )

    timestamp = datetime.now(timezone.utc)
    stamp = timestamp.strftime("%Y%m%dT%H%M%SZ")
    name = (
        f"{stamp}-{safe_slug(args.carrier)}-"
        f"{safe_slug(args.method)}-observation.txt"
    )
    path = out / name

    fields: list[tuple[str, str]] = [
        ("timestamp_utc", timestamp.isoformat().replace("+00:00", "Z")),
        ("git_commit", git_commit()),
        ("carrier", args.carrier),
        ("role", "observation"),
        ("result", "PASS"),
        ("method", args.method.strip()),
        (
            "evidence_level",
            "OPERATOR_RECORDED_OBSERVATION",
        ),
    ]

    if args.setup_latency_ms is not None:
        fields.append(
            ("setup_latency_ms", format(args.setup_latency_ms, ".12g"))
        )
    if args.range_m is not None:
        fields.append(("range_m", format(args.range_m, ".12g")))
    if args.energy_joules is not None:
        fields.extend(
            [
                (
                    "energy_joules",
                    format(args.energy_joules, ".12g"),
                ),
                ("energy_method", args.energy_method.strip()),
            ]
        )
    if args.concurrent_internet is not None:
        fields.append(
            (
                "concurrent_internet",
                "true" if args.concurrent_internet else "false",
            )
        )
    if args.note:
        fields.append(("operator_note", args.note.strip()))

    path.write_text(
        "".join(f"{key}={value}\n" for key, value in fields),
        encoding="utf-8",
    )
    return path


def self_test() -> None:
    import tempfile

    with tempfile.TemporaryDirectory() as tmp:
        parser = build_parser()
        args = parser.parse_args(
            [
                tmp,
                "--carrier",
                "rfcomm",
                "--method",
                "external-meter",
                "--setup-latency-ms",
                "87.5",
                "--range-m",
                "4.2",
                "--energy-joules",
                "1.25",
                "--energy-method",
                "usb-power-meter",
                "--note",
                "table-top run",
            ]
        )
        path = write_observation(args)
        text = path.read_text(encoding="utf-8")
        assert "carrier=rfcomm" in text
        assert "role=observation" in text
        assert "result=PASS" in text
        assert "setup_latency_ms=87.5" in text
        assert "range_m=4.2" in text
        assert "energy_joules=1.25" in text
        assert "energy_method=usb-power-meter" in text

        failed = False
        try:
            bad = parser.parse_args(
                [
                    tmp,
                    "--carrier",
                    "ble_gatt",
                    "--method",
                    "meter",
                    "--energy-joules",
                    "1",
                ]
            )
            write_observation(bad)
        except ValueError:
            failed = True
        assert failed

    print("PHYSICAL_OBSERVATION_WRITER_SELF_TEST_PASS")


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description=(
            "Write validated operator-recorded physical observations. "
            "The tool does not infer or fabricate measurements."
        )
    )
    parser.add_argument(
        "evidence_dir",
        nargs="?",
        help="Campaign evidence directory.",
    )
    parser.add_argument(
        "--carrier",
        choices=sorted(CARRIERS),
    )
    parser.add_argument(
        "--method",
        help="Measurement/setup method.",
    )
    parser.add_argument(
        "--setup-latency-ms",
        type=lambda value: nonnegative_float(
            "setup_latency_ms",
            value,
        ),
    )
    parser.add_argument(
        "--range-m",
        type=lambda value: positive_float("range_m", value),
    )
    parser.add_argument(
        "--energy-joules",
        type=lambda value: positive_float(
            "energy_joules",
            value,
        ),
    )
    parser.add_argument(
        "--energy-method",
    )
    parser.add_argument(
        "--concurrent-internet",
        type=parse_bool,
    )
    parser.add_argument(
        "--note",
    )
    parser.add_argument(
        "--self-test",
        action="store_true",
    )
    return parser


def main() -> int:
    parser = build_parser()
    args = parser.parse_args()

    if args.self_test:
        self_test()
        return 0

    if not args.evidence_dir:
        parser.error("evidence_dir is required")
    if not args.carrier:
        parser.error("--carrier is required")
    if not args.method or not args.method.strip():
        parser.error("--method is required")

    try:
        path = write_observation(args)
    except ValueError as error:
        parser.error(str(error))

    print(
        "PHYSICAL_OBSERVATION_WRITTEN "
        f"carrier={args.carrier} path={path}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
