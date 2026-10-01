#!/usr/bin/env python3
"""Assess sanpham3 physical-gate evidence without closing gates automatically."""

from __future__ import annotations

import argparse
import json
import math
import tempfile
from pathlib import Path
from typing import Iterable

ROLE_REQUIREMENTS = {
    "nfc_hce": {"reader", "hce"},
    "ble_gatt": {"client", "server"},
    "rfcomm": {"client", "server"},
    "local_only_hotspot": {"client", "server"},
}

MEASUREMENT_REQUIREMENTS = {
    "nfc_hce": {
        "reader": {
            "max_transceive_length",
            "benchmark_rounds",
            "benchmark_payload_bytes",
            "benchmark_elapsed_ns",
            "round_trip_useful_bps",
        },
    },
    "ble_gatt": {
        "client": {
            "g8_plus_benchmark_total_ms",
            "benchmark_rounds",
            "benchmark_payload_bytes",
            "benchmark_elapsed_ns",
            "benchmark_rtt_median_ns",
            "benchmark_rtt_p95_ns",
            "benchmark_one_way_useful_bps",
        },
    },
    "rfcomm": {
        "client": {
            "benchmark_rounds",
            "benchmark_payload_bytes",
            "benchmark_elapsed_ns",
            "benchmark_rtt_median_ns",
            "benchmark_rtt_p95_ns",
            "benchmark_one_way_useful_bps",
        },
    },
    "local_only_hotspot": {
        "server": {
            "hotspot_startup_ms",
            "benchmark_rounds",
            "benchmark_payload_bytes",
        },
        "client": {
            "network_join_ms",
            "g8_ms",
            "benchmark_rounds",
            "benchmark_payload_bytes",
            "benchmark_elapsed_ns",
            "benchmark_rtt_median_ns",
            "benchmark_rtt_p95_ns",
            "benchmark_one_way_useful_bps",
        },
    },
}

PHYSICAL_EXTRA_REQUIREMENTS = {
    "nfc_hce": {
        "failure_rate": set(),
    },
    "ble_gatt": {
        "setup_latency": {"setup_latency_ms"},
        "energy": {"energy_joules", "energy_method"},
        "failure_rate": set(),
    },
    "rfcomm": {
        "setup_latency": {"setup_latency_ms"},
        "range": {"range_m"},
        "energy": {"energy_joules", "energy_method"},
        "failure_rate": set(),
    },
    "local_only_hotspot": {
        "concurrent_internet": {"concurrent_internet"},
        "energy": {"energy_joules", "energy_method"},
    },
}


def parse_kv_file(path: Path) -> dict[str, str]:
    fields: dict[str, str] = {"_path": str(path)}
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
    return fields


def txt_files(paths: Iterable[str]) -> list[Path]:
    found: set[Path] = set()
    for raw in paths:
        path = Path(raw)
        if path.is_file() and path.suffix.lower() == ".txt":
            found.add(path)
        elif path.is_dir():
            found.update(p for p in path.rglob("*.txt") if p.is_file())
    return sorted(found)


def physical_serials(records: list[dict[str, str]]) -> set[str]:
    serials: set[str] = set()
    for record in records:
        serial = record.get("serial")
        qemu = record.get("qemu")
        if serial and qemu is not None and qemu != "1":
            serials.add(serial)
    return serials


def pass_records(
    records: list[dict[str, str]],
    carrier: str,
    role: str,
) -> list[dict[str, str]]:
    return [
        record
        for record in records
        if record.get("carrier") == carrier
        and record.get("role") == role
        and record.get("result") == "PASS"
    ]


def fail_records(
    records: list[dict[str, str]],
    carrier: str,
    role: str,
) -> list[dict[str, str]]:
    return [
        record
        for record in records
        if record.get("carrier") == carrier
        and record.get("role") == role
        and record.get("result") == "FAIL"
    ]


def field_value_valid(key: str, value: str | None) -> bool:
    if value is None:
        return False
    value = value.strip()
    if not value:
        return False

    if key == "concurrent_internet":
        return value.lower() in {"true", "false"}

    numeric_keys = {
        "max_transceive_length",
        "benchmark_rounds",
        "benchmark_payload_bytes",
        "benchmark_elapsed_ns",
        "round_trip_useful_bps",
        "g8_plus_benchmark_total_ms",
        "benchmark_rtt_median_ns",
        "benchmark_rtt_p95_ns",
        "benchmark_one_way_useful_bps",
        "hotspot_startup_ms",
        "network_join_ms",
        "g8_ms",
        "setup_latency_ms",
        "range_m",
        "energy_joules",
    }
    if key in numeric_keys:
        try:
            parsed = float(value)
        except ValueError:
            return False
        if not math.isfinite(parsed):
            return False
        if key in {"network_join_ms", "g8_ms", "setup_latency_ms"}:
            return parsed >= 0.0
        return parsed > 0.0

    return True


def record_has_fields(
    records: list[dict[str, str]],
    required: set[str],
) -> bool:
    return any(
        all(field_value_valid(key, record.get(key)) for key in required)
        for record in records
    )


def observation_present(
    records: list[dict[str, str]],
    carrier: str,
    required: set[str],
) -> bool:
    candidates = [
        record
        for record in records
        if record.get("carrier") == carrier
        and record.get("result") == "PASS"
    ]
    return record_has_fields(candidates, required)


def trace_candidates(
    roots: list[Path],
    records: list[dict[str, str]],
    physical: set[str],
) -> dict[str, dict]:
    result: dict[str, dict] = {}

    for mode, label, replay_marker in [
        ("audio", "real_recorded_impulse_response_replay", "F3_ACOUSTIC_REPLAY"),
        (
            "accelerometer",
            "recorded_sensor_trace_replay",
            "F3_VIBRATION_REPLAY",
        ),
    ]:
        candidates = []
        for record in records:
            if record.get("mode") != mode:
                continue
            if record.get("evidence_level") != "ANDROID_RUNTIME_CAPTURE":
                continue
            serial = record.get("serial")
            expected = record.get("expected_hex")
            if not serial or serial not in physical:
                continue
            if not expected or expected == "none":
                continue

            metadata_path = Path(record["_path"])
            replay_path = metadata_path.parent / "replay.txt"
            replay_ok = False
            if replay_path.is_file():
                replay_text = replay_path.read_text(
                    encoding="utf-8",
                    errors="replace",
                )
                replay_ok = (
                    replay_marker in replay_text
                    and "bit_errors=0" in replay_text
                )

            candidates.append(
                {
                    "serial": serial,
                    "metadata": str(metadata_path),
                    "replay": str(replay_path),
                    "zero_ber": replay_ok,
                    "expected_hex": expected,
                }
            )

        ready = any(item["zero_ber"] for item in candidates)
        result[label] = {
            "candidate_ready": ready,
            "candidates": candidates,
            "missing": [] if ready else [
                "physical-device trace metadata",
                "known expected_hex",
                "replay.txt with bit_errors=0",
            ],
        }

    return result


def carrier_readiness(
    records: list[dict[str, str]],
    physical: set[str],
) -> dict[str, dict]:
    result: dict[str, dict] = {}

    for carrier, required_roles in ROLE_REQUIREMENTS.items():
        role_status = {}
        all_roles_pass = True

        for role in sorted(required_roles):
            passed = pass_records(records, carrier, role)
            failed = fail_records(records, carrier, role)
            role_status[role] = {
                "pass_records": len(passed),
                "fail_records": len(failed),
            }
            if not passed:
                all_roles_pass = False

        interoperability_ready = len(physical) >= 2 and all_roles_pass

        measurement_missing = []
        measurement_requirements = MEASUREMENT_REQUIREMENTS.get(
            carrier,
            {},
        )
        for role, required_fields in measurement_requirements.items():
            passed = pass_records(records, carrier, role)
            if not record_has_fields(passed, required_fields):
                measurement_missing.append(
                    {
                        "role": role,
                        "fields": sorted(required_fields),
                    }
                )

        extras = {}
        for name, required_fields in PHYSICAL_EXTRA_REQUIREMENTS.get(
            carrier,
            {},
        ).items():
            if name == "failure_rate":
                # A single successful pair cannot characterize reliability.
                # Require at least two recorded outcomes for every required
                # role before calling failure-rate evidence present.
                present = all(
                    role_status[role]["pass_records"]
                    + role_status[role]["fail_records"]
                    >= 2
                    for role in required_roles
                )
            else:
                present = observation_present(
                    records,
                    carrier,
                    required_fields,
                )
            extras[name] = present

        result[carrier] = {
            "physical_device_count": len(physical),
            "roles": role_status,
            "interoperability_candidate_ready": interoperability_ready,
            "measurement_core_candidate_ready": (
                interoperability_ready and not measurement_missing
            ),
            "measurement_core_missing": measurement_missing,
            "physical_extras": extras,
            "full_measurement_candidate_ready": (
                interoperability_ready
                and not measurement_missing
                and all(extras.values())
            ),
        }

    return result


def summarize(paths: list[str]) -> dict:
    files = txt_files(paths)
    records = [parse_kv_file(path) for path in files]
    physical = physical_serials(records)
    roots = [
        Path(raw) if Path(raw).is_dir() else Path(raw).parent
        for raw in paths
    ]

    return {
        "physical_serials": sorted(physical),
        "physical_device_count": len(physical),
        "recorded_trace_gates": trace_candidates(
            roots,
            records,
            physical,
        ),
        "carrier_gates": carrier_readiness(records, physical),
        "provenance_note": (
            "This report never closes frontier gates. It only checks whether "
            "the evidence bundle contains conservative candidate prerequisites."
        ),
    }


def render_text(summary: dict) -> str:
    lines = [summary["provenance_note"]]
    lines.append(
        "physical_devices="
        + str(summary["physical_device_count"])
        + " serials="
        + ",".join(summary["physical_serials"])
    )

    lines.append("")
    lines.append("Recorded trace gates:")
    for gate, state in summary["recorded_trace_gates"].items():
        lines.append(
            f"  {gate}: candidate_ready="
            f"{str(state['candidate_ready']).lower()}"
        )
        for missing in state["missing"]:
            lines.append(f"    missing: {missing}")

    lines.append("")
    lines.append("Carrier gates:")
    for carrier, state in summary["carrier_gates"].items():
        lines.append(
            f"  {carrier}: interoperability_candidate_ready="
            f"{str(state['interoperability_candidate_ready']).lower()} "
            f"measurement_core_candidate_ready="
            f"{str(state['measurement_core_candidate_ready']).lower()} "
            f"full_measurement_candidate_ready="
            f"{str(state['full_measurement_candidate_ready']).lower()}"
        )
        for role, role_state in state["roles"].items():
            lines.append(
                f"    role={role} pass={role_state['pass_records']} "
                f"fail={role_state['fail_records']}"
            )
        for missing in state["measurement_core_missing"]:
            lines.append(
                "    missing measurement fields "
                f"role={missing['role']}: "
                + ",".join(missing["fields"])
            )
        for name, present in state["physical_extras"].items():
            if not present:
                lines.append(f"    missing physical extra: {name}")

    return "\n".join(lines)


def self_test() -> None:
    with tempfile.TemporaryDirectory() as tmp:
        root = Path(tmp)

        (root / "device-a-metadata.txt").write_text(
            "serial=A\nqemu=0\n",
            encoding="utf-8",
        )
        (root / "device-b-metadata.txt").write_text(
            "serial=B\nqemu=0\n",
            encoding="utf-8",
        )

        for role in ("client", "server"):
            fields = [
                "carrier=rfcomm",
                f"role={role}",
                "result=PASS",
            ]
            if role == "client":
                fields += [
                    "benchmark_rounds=32",
                    "benchmark_payload_bytes=1024",
                    "benchmark_elapsed_ns=1000000",
                    "benchmark_rtt_median_ns=1000",
                    "benchmark_rtt_p95_ns=2000",
                    "benchmark_one_way_useful_bps=1000000",
                ]
            (root / f"rfcomm-{role}.txt").write_text(
                "\n".join(fields) + "\n",
                encoding="utf-8",
            )

        trace = root / "trace-audio"
        trace.mkdir()
        (trace / "metadata.txt").write_text(
            "\n".join(
                [
                    "serial=A",
                    "mode=audio",
                    "expected_hex=aa55",
                    "evidence_level=ANDROID_RUNTIME_CAPTURE",
                ]
            )
            + "\n",
            encoding="utf-8",
        )
        (trace / "replay.txt").write_text(
            "F3_ACOUSTIC_REPLAY bit_errors=0\n",
            encoding="utf-8",
        )

        (root / "invalid-energy.txt").write_text(
            "carrier=rfcomm\nrole=observation\nresult=PASS\n"
            "energy_joules=\nenergy_method=meter\n",
            encoding="utf-8",
        )

        result = summarize([str(root)])
        assert result["physical_device_count"] == 2
        assert result["recorded_trace_gates"][
            "real_recorded_impulse_response_replay"
        ]["candidate_ready"]
        assert result["carrier_gates"]["rfcomm"][
            "interoperability_candidate_ready"
        ]
        assert result["carrier_gates"]["rfcomm"][
            "measurement_core_candidate_ready"
        ]
        assert not result["carrier_gates"]["rfcomm"][
            "full_measurement_candidate_ready"
        ]

    print("PHYSICAL_GATE_READINESS_SELF_TEST_PASS")


def main() -> int:
    parser = argparse.ArgumentParser(
        description=(
            "Assess sanpham3 physical-gate evidence conservatively. "
            "The report never closes gates automatically."
        )
    )
    parser.add_argument(
        "paths",
        nargs="*",
        help="Evidence files/directories to scan recursively.",
    )
    parser.add_argument(
        "--json",
        action="store_true",
        help="Emit JSON instead of text.",
    )
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="Run deterministic self-test and exit.",
    )
    args = parser.parse_args()

    if args.self_test:
        self_test()
        return 0

    if not args.paths:
        parser.error("provide at least one evidence file/directory")

    result = summarize(args.paths)
    if args.json:
        print(json.dumps(result, indent=2, sort_keys=True))
    else:
        print(render_text(result))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
