#!/usr/bin/env python3
# Licensed to the Apache Software Foundation (ASF) under one
# or more contributor license agreements.  See the NOTICE file
# distributed with this work for additional information
# regarding copyright ownership.  The ASF licenses this file
# to you under the Apache License, Version 2.0 (the
# "License"); you may not use this file except in compliance
# with the License.  You may obtain a copy of the License at
#
#     http://www.apache.org/licenses/LICENSE-2.0
#
# Unless required by applicable law or agreed to in writing,
# software distributed under the License is distributed on an
# "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
# KIND, either express or implied.  See the License for the
# specific language governing permissions and limitations
# under the License.
"""Repeat the Unix release real-Host benchmark, retaining every sample and failure.

Build once: cargo test --release -p maka-agent-native --lib --no-run
Run: python3 scripts/agent-scale.py --binary target/release/deps/maka_agent_native-HASH
The binary must be the test executable printed by Cargo, not a debug build.
"""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--binary", type=Path, required=True)
parser.add_argument("--baseline", type=Path, help="alternate candidate and baseline in the same run")
parser.add_argument("--prompt-bytes", type=int)
parser.add_argument("--delta-bytes", type=int)
parser.add_argument("--agents", type=int, nargs="+", default=[10, 100, 1000])
parser.add_argument("--repeat", type=int, default=5)
parser.add_argument("--deltas", type=int, default=32)
parser.add_argument("--scenario", choices=["barrier", "flowing"], default="barrier")
parser.add_argument("--waves", type=int, default=1)
parser.add_argument("--mixed", action="store_true")
parser.add_argument("--output", type=Path, default=Path("/tmp/maka-agent-scale.json"))
args = parser.parse_args()
if min(args.agents + [args.repeat, args.deltas, args.waves] + [n for n in (args.prompt_bytes, args.delta_bytes) if n is not None]) < 1:
    parser.error("all workload sizes and repetitions must be positive")
if args.scenario == "barrier" and (args.mixed or args.waves != 1):
    parser.error("mixed sizes and repeated waves require --scenario flowing")
if args.scenario == "flowing" and (args.prompt_bytes is not None or args.delta_bytes is not None):
    parser.error("flowing sizes are selected by --mixed")
binaries = {"candidate": args.binary.resolve()}
if args.baseline:
    binaries["baseline"] = args.baseline.resolve()
args.output.parent.mkdir(parents=True, exist_ok=True)
report = {
    "recorded_at": datetime.datetime.now(datetime.timezone.utc).isoformat(),
    "machine": platform.platform(), "logical_cpus": os.cpu_count(),
    "load_average": os.getloadavg(),
    "binaries": {name: {"path": str(path), "sha256": hashlib.file_digest(path.open("rb"), "sha256").hexdigest()} for name, path in binaries.items()},
    "runs": [],
    "sizes": {"agents": args.agents, "prompt_bytes": args.prompt_bytes or 5, "delta_bytes": args.delta_bytes or 6, "deltas": args.deltas},
    "workload": (
        "real Host/native owner/native Chat adapter; all HTTP held until every agent arrives; 2ms paced SSE; durable output verified"
        if args.scenario == "barrier" else
        "real Host/native owner/native Chat adapter; all Agents admitted before releasing responses; flowing HTTP overlap; exact durable output, retirement and recovery verified"
    ),
    "scenario": args.scenario, "waves": args.waves, "mixed_sizes": args.mixed,
    "scope": "includes in-process local HTTP server; excludes TUI rendering and external model latency",
    "cpu_scope": "model admission through stream settlement; memory is process peak across the entire run",
}
if args.scenario == "flowing":
    report["sizes"] = {"agents": args.agents, "prompt_bytes": [1024, 8192, 65536, 262144] if args.mixed else [32],
                       "delta_bytes": [16, 256, 2048, 8192] if args.mixed else [6], "deltas": args.deltas}
for repetition in range(args.repeat):
    for agents in args.agents:
        variants = list(binaries)
        if repetition % 2:
            variants.reverse()
        for variant in variants:
            env = dict(os.environ)
            for name in ("MAKA_SCALE_MEMORY_PROFILE", "MallocStackLogging", "MallocStackLoggingNoCompact", "MAKA_SCALE_PROMPT_BYTES", "MAKA_SCALE_DELTA_BYTES", "MAKA_SCALE_WAVES", "MAKA_SCALE_MIXED"):
                env.pop(name, None)
            env.update(MAKA_SCALE_AGENTS=str(agents), MAKA_SCALE_DELTAS=str(args.deltas))
            for name, value in (("MAKA_SCALE_PROMPT_BYTES", args.prompt_bytes), ("MAKA_SCALE_DELTA_BYTES", args.delta_bytes)):
                if value is not None:
                    env[name] = str(value)
            if args.scenario == "flowing":
                env["MAKA_SCALE_WAVES"] = str(args.waves)
                if args.mixed:
                    env["MAKA_SCALE_MIXED"] = "1"
            test = "owner::tests::scale::real_host_agent_scale" if args.scenario == "barrier" else "owner::tests::scale::flowing::real_host_agent_memory"
            timeout = 660 * args.waves
            command = [str(binaries[variant]), test, "--ignored", "--exact", "--nocapture", "--test-threads=1"]
            started = datetime.datetime.now(datetime.timezone.utc).isoformat()
            load = os.getloadavg()
            try:
                result = subprocess.run(command, env=env, capture_output=True, text=True, timeout=timeout)
                marker = "MAKA_AGENT_SCALE "
                payloads = [line.split(marker, 1)[1] for line in result.stdout.splitlines() if marker in line]
                row = json.loads(payloads[0]) if result.returncode == 0 and len(payloads) == 1 else {
                    "agents": agents, "errors": 1, "exit_code": result.returncode,
                    "stdout": result.stdout[-16000:], "stderr": result.stderr[-16000:],
                }
            except subprocess.TimeoutExpired:
                row = {"agents": agents, "errors": 1, "timeout_seconds": timeout}
            row.update(variant=variant, repetition=repetition + 1, started_at=started, load_average=load)
            report["runs"].append(row)
            args.output.write_text(json.dumps(report, indent=2) + "\n")
            print(json.dumps(row), flush=True)
            if row["errors"]:
                raise SystemExit(f"benchmark failed; evidence: {args.output}")
report["summary"] = {}
for variant in binaries:
    for agents in args.agents:
        rows = [r for r in report["runs"] if r["agents"] == agents and r["variant"] == variant]
        def distribution(values):
            return {"median": statistics.median(values), "min": min(values), "max": max(values)}

        if args.scenario == "barrier":
            summary = {
                key: distribution([r[key] for r in rows])
                for key in ["all_active_ms", "stream_and_settle_ms", "durable_input_deltas_per_second", "cpu_seconds", "peak_rss_bytes"]
            }
            summary["foreground_query_p99_ms"] = distribution([r["foreground_query_ms"]["p99"] for r in rows])
        else:
            summary = {"peak_rss_bytes": distribution([r["peak_rss_bytes"] for r in rows]), "waves": []}
            for index in range(args.waves):
                waves = [r["waves"][index] for r in rows]
                summary["waves"].append({
                    **{key: distribution([w[key] for w in waves]) for key in ["all_admitted_ms", "stream_and_settle_ms", "cpu_seconds", "peak_http_overlap", "verified_output_bytes"]},
                    "foreground_query_p99_ms": distribution([w["foreground_query_ms"]["p99"] for w in waves]),
                })
        for key, values in {
            "cancel_and_settle_ms": [r["cancellation"]["cancel_and_settle_ms"] for r in rows],
            "recovery_ms": [r["cancellation"]["recovery_ms"] for r in rows],
        }.items():
            summary[key] = distribution(values)
        report["summary"].setdefault(variant, {})[str(agents)] = summary
args.output.write_text(json.dumps(report, indent=2) + "\n")
print(f"Evidence: {args.output}")
