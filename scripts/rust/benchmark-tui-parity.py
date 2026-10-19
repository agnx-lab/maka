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
"""Compare real TUI frames and responsiveness, without building either revision.

Build the integration fixture once, preserving both CLI executables beforehand:
  cargo test -p maka-cli --no-default-features --test integration --no-run
  benchmark-tui-parity.py --fixture target/debug/deps/integration-... \
    --baseline /tmp/maka-before --candidate target/debug/maka \
    --bundle /tmp/maka-product-bundle \
    --runs 5 --output /tmp/maka-tui-parity
Use --case concurrent for sustained model streaming with simultaneous typing and
wheel input. Its intermediate frames are recorded but only stable final frames
are compared across runs. The fixture remains strict about input during streaming.
Only a baseline's dedicated responsiveness assertion may fail after all fragments,
both complete-content copies, the final checkpoint and cleanup succeed. Its full
schema-1 outcome is retained separately; candidate failures and all content or
lifecycle failures are fatal. Frame/work comparisons remain strict.
Its final measurement covers the whole stream and verification, including the
interleaved input samples; these overlapping measurements must not be summed.

The Rust fixture uses the terminal emulator's complete synchronized frames, not
ANSI-stripped output. Raw snapshots and ANSI are retained. Known animation frames,
the conversation pane's standalone elapsed field, the final footer's token-rate
number, and invisible cursor positions are excluded from cross-run equality.
Elapsed fields retain their row, pane boundaries and width; headers, nearby text
and Context values remain compared. Every exclusion is recorded. Colors/hit rectangles need
the complementary renderer Buffer/geometry tests. Linux UI-process CPU excludes
the separate Host, model server, and fixture. First-visible latency includes PTY
observation. CPU and ANSI bytes cover the longer observation_ms window through
the final checkpoint, including 200 ms of follow-up draining after first visibility.
Linux ui_rss_bytes is checkpoint VmRSS; ui_peak_rss_bytes is process-wide VmHWM
since startup through that checkpoint. Both use bytes, exclude the Host/fixture,
and are null elsewhere. Memory snapshots and cumulative peaks must not be summed.
Use the existing debug profile for both executables; this tool never builds or
changes optimization levels. Process-owner cleanup currently requires Linux /proc.
"""

import argparse
import difflib
import json
import os
from pathlib import Path
import re
import signal
import statistics
import subprocess
import sys
import tempfile
import time

TEST = ("terminal::bundled_product::chat::performance::parity::"
        "default_product_preserves_frames_and_responsiveness")
CONCURRENT_TEST = ("terminal::bundled_product::chat::performance::parity::"
                   "default_product_accepts_input_and_scroll_during_continuous_stream")
PREFIX = "MAKA_TUI_PARITY "
RESPONSIVENESS_PREFIX = "MAKA_TUI_RESPONSIVENESS "
RESPONSIVENESS_FAILURE = "MAKA_TUI_RESPONSIVENESS_STRICT_FAILURE"
# Exact frames from plugins/shared/motion.ts; do not mask arbitrary punctuation.
ANIMATIONS = ("o_o", ">_>", "o_-", "<_<", "-_-", "⣀⡀ ", "⣤  ", "⠰⠆ ",
              " ⠛ ", " ⠰⠆", "  ⣤", " ⢀⣀", "⢁", "⡈", "⠔", "⠢")
ANIMATION = re.compile("|".join(re.escape(value) for value in ANIMATIONS))
ELAPSED = re.compile(
    r"^(?P<prefix>[^│]*│ +)(?P<slot>(?:[0-9]+h )?(?:[0-9]+m )?[0-9]+s *)(?P<edge>[│┃])$")
FOOTER_RATE = re.compile(
    r"\bContext [^│┃\n]* · (?P<rate>[0-9]+(?:\.[0-9]+)?)(?= tok/s(?: *[│┃])? *$)")


def normalized(record):
    frame = json.loads(json.dumps(record["frame"]))
    exclusions = []
    lines = frame["screen"].split("\n")
    timed = record["checkpoint"].startswith(("11-", "12-", "13-"))
    for row, line in enumerate(lines):
        def animation(match):
            exclusions.append(dict(kind="animation", row=row, start=match.start(),
                                   value=match.group()))
            return "~" * len(match.group())
        line = ANIMATION.sub(animation, line)
        if timed:
            elapsed = ELAPSED.fullmatch(line)
            if elapsed:
                slot = elapsed.group("slot")
                exclusions.append(dict(kind="elapsed", row=row, start=elapsed.start("slot"),
                                       value=slot))
                # Keep a marker so a missing elapsed field cannot equal a blank
                # pane row. Equal-width slots also absorb 9s/10s padding changes
                # without moving the label's leading padding, either pane edge
                # or any other row.
                line = elapsed.group("prefix") + "~".ljust(len(slot)) + elapsed.group("edge")
            if row == frame["size"]["rows"] - 1:
                rate = FOOTER_RATE.search(line)
                if rate:
                    exclusions.append(dict(kind="output_token_rate", row=row,
                                           start=rate.start("rate"), value=rate.group("rate")))
                    line = line[:rate.start("rate")] + "<rate>" + line[rate.end("rate"):]
        lines[row] = line
    frame["screen"] = "\n".join(lines)
    if not frame["cursor"]["visible"]:
        exclusions.append(dict(kind="invisible_cursor_position", value=frame["cursor"]))
        frame["cursor"] = {"visible": False}
    return frame, exclusions


def records(output):
    parsed = []
    for line in output.splitlines():
        at = line.find(PREFIX)
        if at >= 0:
            parsed.append(json.loads(line[at + len(PREFIX):]))
    names = [record["checkpoint"] for record in parsed]
    if len(set(names)) != len(names):
        raise ValueError("duplicate checkpoint names")
    if not names or names[0] != "01-history-ready" or names[-1] != "13-completed":
        raise ValueError("fixture did not complete its real UI/model work")
    return parsed


def fixture_result(output, returncode, test, revision):
    rows = records(output)
    markers = [json.loads(line[line.index(RESPONSIVENESS_PREFIX) + len(RESPONSIVENESS_PREFIX):])
               for line in output.splitlines() if RESPONSIVENESS_PREFIX in line]
    responsiveness = None
    if test == CONCURRENT_TEST:
        if len(markers) != 1:
            raise ValueError("concurrent fixture requires one complete responsiveness outcome")
        responsiveness = markers[0]

        def require(condition, reason):
            if not condition:
                raise ValueError(f"invalid responsiveness outcome: {reason}")

        require(isinstance(responsiveness, dict), "expected schema-1 object")
        require(output.index(RESPONSIVENESS_PREFIX) > output.rfind(PREFIX),
                "outcome follows the final checkpoint")
        for key, value in dict(schema=1, case="concurrent", stream_fragments=160,
                               completed_fragments=160, draft_verified=True,
                               source_copy_verified=True, checkpoint="13-completed",
                               cleanup_complete=True).items():
            require(type(responsiveness.get(key)) is type(value)
                    and responsiveness[key] == value, key)
        expected_names = (["01-history-ready"]
                          + [f"concurrent-type-{character}" for character in "并发输入"]
                          + ["concurrent-wheel-up", "13-completed"])
        require([row["checkpoint"] for row in rows] == expected_names, "complete checkpoint sequence")
        work = rows[-1]["work"]
        require(work.get("fragments") == 160 and work.get("verified_complete_response") is True
                and all(type(work.get(key)) is int and work[key] > 0
                        for key in ("response_bytes", "verified_draft_bytes")), "complete final work")
        conditions = responsiveness.get("conditions", {})
        require(isinstance(conditions, dict), "conditions")
        inputs = conditions.get("inputs")
        require(isinstance(inputs, list) and len(inputs) == 4, "four input conditions")
        failures = []

        def condition_ok(condition, observed):
            require(isinstance(condition, dict), "condition object")
            sent, response = condition.get("sent_fragments"), condition.get("response_fragments")
            require(type(sent) is int and 0 <= sent <= 160, "sent fragment count")
            require((type(response) is int and sent <= response <= 160) if observed
                    else response is None, "response fragment count")
            ok = observed and sent < 160 and response < 160
            require(condition.get("ok") is ok, "condition verdict")
            return ok

        for character, condition in zip("并发输入", inputs):
            ok = condition_ok(condition, True)
            require(condition.get("character") == character, "input character")
            require(condition.get("sent_while_streaming") is (condition["sent_fragments"] < 160)
                    and condition.get("response_while_streaming")
                    is (condition["response_fragments"] < 160), "input streaming verdict")
            if not ok:
                failures.append(f"input:{character}")
        for name in ("native_wheel", "guest_feedback"):
            condition = conditions.get(name, {})
            require(isinstance(condition, dict) and type(condition.get("observed")) is bool,
                    f"{name} observation")
            if not condition_ok(condition, condition["observed"]):
                failures.append(name)
        require(responsiveness.get("failures") == failures, "exact failed conditions")
        require(responsiveness.get("ok") is (not failures), "overall verdict")
    elif markers:
        raise ValueError("unexpected responsiveness outcome for regular fixture")

    baseline_failure = False
    if returncode:
        # A Rust assertion exits 101. Accept only this one known performance
        # assertion after the complete schema above, never an arbitrary panic.
        expected_panic = re.compile(
            rf"^thread '{re.escape(CONCURRENT_TEST)}'(?: \([0-9]+\))? panicked at [^\n]+:\n"
            rf"{RESPONSIVENESS_FAILURE}$", re.MULTILINE)
        baseline_failure = (revision == "baseline" and test == CONCURRENT_TEST and returncode == 101
                            and responsiveness["ok"] is False
                            and output.count(RESPONSIVENESS_FAILURE) == 1
                            and output.count(" panicked at ") == 1
                            and expected_panic.search(output) is not None
                            and "test result: FAILED. 0 passed; 1 failed;" in output)
        if not baseline_failure:
            raise ValueError(f"fixture exited {returncode}")
    elif " panicked at " in output:
        raise ValueError("fixture logged a panic despite a successful exit")
    elif responsiveness is not None and responsiveness["ok"] is not True:
        raise ValueError("failed responsiveness outcome without the strict assertion")
    return rows, dict(fixture_exit_code=returncode, responsiveness=responsiveness,
                      baseline_responsiveness_failure=baseline_failure)


def compare(reference, candidate, directory):
    errors = []
    if [row["checkpoint"] for row in reference] != [row["checkpoint"] for row in candidate]:
        return ["checkpoint sequence differs"]
    for left, right in zip(reference, candidate):
        name = left["checkpoint"]
        before, before_exclusions = normalized(left)
        after, after_exclusions = normalized(right)
        (directory / f"{name}-exclusions.json").write_text(json.dumps(
            dict(baseline=before_exclusions, candidate=after_exclusions),
            ensure_ascii=False, indent=2) + "\n")
        if left["work"] != right["work"]:
            errors.append(f"{name}: completed work differs")
        if left.get("compare_frame", True) != right.get("compare_frame", True):
            errors.append(f"{name}: frame comparison policy differs")
        if left.get("compare_frame", True) and before != after:
            errors.append(f"{name}: frame differs")
            difference = "".join(difflib.unified_diff(
                json.dumps(before, ensure_ascii=False, indent=2).splitlines(True),
                json.dumps(after, ensure_ascii=False, indent=2).splitlines(True),
                fromfile="baseline", tofile="candidate"))
            (directory / f"{name}.diff").write_text(difference)
    return errors


def summary(samples):
    groups = {}
    for sample in samples:
        for record in sample["records"]:
            groups.setdefault((sample["revision"], record["checkpoint"]), []).append(record["metrics"])
    result = []
    for (revision, checkpoint), rows in sorted(groups.items()):
        metrics = {}
        for key in ("latency_ms", "observation_ms", "ui_cpu_ms", "ansi_bytes",
                    "ui_rss_bytes", "ui_peak_rss_bytes"):
            values = [row[key] for row in rows if row[key] is not None]
            metrics[key] = (dict(n=len(values), median=statistics.median(values),
                                 minimum=min(values), maximum=max(values)) if values else None)
        result.append(dict(revision=revision, checkpoint=checkpoint, metrics=metrics))
    return result


def process_identity(pid):
    try:
        proc = Path(f"/proc/{pid}")
        stat = (proc / "stat").read_text()
        fields = stat[stat.rfind(")") + 1:].split()
        if fields[0] == "Z":
            return None
        return dict(pid=pid, start_ticks=int(fields[19]),
                    argv=(proc / "cmdline").read_bytes().decode().rstrip("\0").split("\0"))
    except (OSError, ValueError, IndexError):
        return None


def owns_process(identity, root, binary, start_ticks=None):
    if identity is None:
        return False
    argv = identity["argv"]
    try:
        return (Path(argv[0]).resolve() == Path(binary).resolve()
                and argv[argv.index("--root") + 1] == str(root)
                and (start_ticks is None or identity["start_ticks"] == start_ticks))
    except (ValueError, IndexError):
        return False


def cleanup(directory, binary):
    """Retire only registered roots and verified independent PTY/Host processes."""
    report = dict(ui=[], hosts=[], signals=[], remaining=[])
    owners_file = directory / "owners.jsonl"
    owners = []
    if owners_file.exists():
        for line in owners_file.read_text().splitlines():
            try:
                owners.append(json.loads(line))
            except ValueError as error:
                report.setdefault("registration_errors", []).append(str(error))
    roots = {owner["host_root"] for owner in owners if owner.get("host_root")}
    verified = {}
    for owner in owners:
        identity = process_identity(owner["ui_pid"])
        owned = owns_process(identity, owner.get("ui_root"), owner["binary"], owner.get("start_ticks"))
        report["ui"].append(dict(owner=owner, identity=identity, verified=owned))
        if owned:
            verified[identity["pid"]] = identity
    for pid in verified:
        try:
            os.kill(pid, signal.SIGTERM)
            report["signals"].append(dict(pid=pid, signal="SIGTERM", kind="registered_ui"))
        except ProcessLookupError:
            pass
    # Use the ordinary Host lifecycle first. The UI is setsid-owned, so killing
    # the test's process group alone would leave both this UI and its Host alive.
    for root in sorted(roots):
        if not Path(root).exists():
            continue
        try:
            retired = subprocess.run([str(binary), "host", "retire", "--root", root,
                                      "--allow-interrupt-active-tasks"], text=True,
                                     stdout=subprocess.PIPE, stderr=subprocess.STDOUT, timeout=10)
            report["hosts"].append(dict(root=root, exit_code=retired.returncode, output=retired.stdout))
        except (OSError, subprocess.TimeoutExpired) as error:
            report["hosts"].append(dict(root=root, error=str(error)))
    # A failed Host may not answer retirement. Only exact registered binary/root
    # argv matches may enter this fallback; never signal by process name alone.
    if Path("/proc").exists():
        for proc in Path("/proc").iterdir():
            if not proc.name.isdigit():
                continue
            identity = process_identity(int(proc.name))
            if identity and "host" in identity["argv"] and any(
                    owns_process(identity, root, binary) for root in roots):
                verified[identity["pid"]] = identity
                try:
                    os.kill(identity["pid"], signal.SIGTERM)
                    report["signals"].append(dict(pid=identity["pid"], signal="SIGTERM", kind="root_host"))
                except ProcessLookupError:
                    pass
    until = time.monotonic() + 2
    while time.monotonic() < until and any(process_identity(pid) for pid in verified):
        time.sleep(0.02)
    for pid, original in verified.items():
        current = process_identity(pid)
        if current and current["start_ticks"] == original["start_ticks"]:
            try:
                os.kill(pid, signal.SIGKILL)
                report["signals"].append(dict(pid=pid, signal="SIGKILL", kind="verified_remaining"))
            except ProcessLookupError:
                pass
    until = time.monotonic() + 2
    while time.monotonic() < until and any(process_identity(pid) for pid in verified):
        time.sleep(0.02)
    report["remaining"] = [current for pid in verified if (current := process_identity(pid))]
    (directory / "cleanup.json").write_text(json.dumps(report, indent=2) + "\n")
    return report


def run_fixture(fixture, test, environment, directory, binary, timeout, revision):
    process = subprocess.Popen([str(fixture), test, "--ignored", "--exact", "--nocapture",
                                "--test-threads=1"], env=environment, text=True,
                               stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    try:
        output, _ = process.communicate(timeout=timeout)
    except (subprocess.TimeoutExpired, KeyboardInterrupt):
        # Clean the independently owned UI and Root before terminating the test.
        cleanup(directory, binary)
        process.terminate()
        try:
            output, _ = process.communicate(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            output, _ = process.communicate()
        (directory / "fixture.log").write_text(output)
        raise
    (directory / "fixture.log").write_text(output)
    if process.returncode:
        cleanup(directory, binary)
    rows, outcome = fixture_result(output, process.returncode, test, revision)
    (directory / "outcome.json").write_text(json.dumps(outcome, ensure_ascii=False, indent=2) + "\n")
    return rows, outcome


def self_test():
    def record(screen, cursor=None):
        return dict(checkpoint="12-stream-0", work={"delta_bytes": 3}, metrics={}, frame=dict(
            screen=screen, size={"cols": 120, "rows": len(screen.split("\n"))},
            cursor=cursor or {"x": 2, "y": 1, "visible": True}, input={}))
    def screen(elapsed, *, rate="0.7", context="≈3", animation="⢁", edge="┃",
               header="Maka · Requested model: fixture-model", body="中文 reply"):
        return "\n".join((
            f"Sidebar │ {header:<60}│",
            f"        │ {animation} {body:<58}│",
            f"        │ {elapsed:<60}{edge}",
            f"        │ fixture-model  Workspace write  Context {context} · {rate} tok/s",
        ))
    before = record(screen("0s"))
    after = record(screen("1s", rate="0.8", animation="⠔"))
    assert normalized(before)[0] == normalized(after)[0]
    for edge in ("│", "┃"):
        short = record(screen("9s", edge=edge))
        long = record(screen("10s", edge=edge))
        assert normalized(short)[0] == normalized(long)[0]
        normalized_frame, exclusions = normalized(long)
        original = long["frame"]["screen"].splitlines()[2]
        normalized_line = normalized_frame["screen"].splitlines()[2]
        assert len(normalized_line) == len(original)
        assert normalized_line.index("│") == original.index("│")
        assert normalized_line[-1] == original[-1]
        excluded = next(value for value in exclusions if value["kind"] == "elapsed")
        assert excluded == dict(kind="elapsed", row=2, start=10,
                                value="10s" + " " * 57)
    assert normalized(before)[0] != normalized(record(screen("1s", header="Wrong model")))[0]
    assert normalized(before)[0] != normalized(record(screen("1s", body="中文 wrong")))[0]
    assert normalized(before)[0] != normalized(record(screen("1s", context="≈4")))[0]
    assert normalized(before)[0] != normalized(record(screen("", edge="┃")))[0]
    assert normalized(before)[0] != normalized(record(screen("1s", edge="│")))[0]
    moved_edge = record(screen("1s").replace("        │ 1s", "       │ 1s"))
    assert normalized(before)[0] != normalized(moved_edge)[0]
    moved_label = record(screen("1s").replace("│ 1s" + " " * 58,
                                             "│  1s" + " " * 57))
    assert normalized(before)[0] != normalized(moved_label)[0]
    assert normalized(before)[0] != normalized(record(
        screen("1s"), {"x": 3, "y": 1, "visible": True}))[0]
    body_rate = record(screen("1s", body="Context ≈3 · 0.7 tok/s"))
    changed_body_rate = record(screen("1s", body="Context ≈3 · 0.8 tok/s"))
    assert normalized(body_rate)[0] != normalized(changed_body_rate)[0]
    _, exclusions = normalized(after)
    rate = next(value for value in exclusions if value["kind"] == "output_token_rate")
    assert rate == dict(kind="output_token_rate", row=3,
                        start=after["frame"]["screen"].splitlines()[3].index("0.8"), value="0.8")
    assert normalized(dict(before, checkpoint="02-draft-08k"))[0] != normalized(
        dict(after, checkpoint="02-draft-08k"))[0]
    assert records(PREFIX + json.dumps(dict(before, checkpoint="01-history-ready")) + "\n" +
                   PREFIX + json.dumps(dict(after, checkpoint="13-completed")))[-1]["checkpoint"] == "13-completed"
    completed_rows = [dict(before, checkpoint=name, work={}) for name in (
        "01-history-ready", "concurrent-type-并", "concurrent-type-发", "concurrent-type-输",
        "concurrent-type-入", "concurrent-wheel-up", "13-completed")]
    completed_rows[-1]["work"] = dict(fragments=160, response_bytes=8192,
                                      verified_draft_bytes=8192, verified_complete_response=True)
    responsiveness = dict(
        schema=1, case="concurrent", ok=False, stream_fragments=160, completed_fragments=160,
        draft_verified=True, source_copy_verified=True, checkpoint="13-completed", cleanup_complete=True,
        conditions=dict(
            inputs=[dict(character=character, sent_fragments=10, response_fragments=20,
                         sent_while_streaming=True, response_while_streaming=True, ok=True)
                    for character in "并发输入"],
            native_wheel=dict(observed=True, sent_fragments=20, response_fragments=30, ok=True),
            guest_feedback=dict(observed=True, sent_fragments=20, response_fragments=160, ok=False)),
        failures=["guest_feedback"])
    assertion = (f"thread '{CONCURRENT_TEST}' (1234) panicked at parity.rs:123:5:\n"
                 f"{RESPONSIVENESS_FAILURE}\n\nFAILED\n"
                 "test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured;")

    def concurrent_output(marker=responsiveness, checkpoints=completed_rows, failure=assertion):
        return ("\n".join(PREFIX + json.dumps(value) for value in checkpoints)
                + "\n" + RESPONSIVENESS_PREFIX + json.dumps(marker) + "\n" + failure)

    rows, outcome = fixture_result(concurrent_output(), 101, CONCURRENT_TEST, "baseline")
    assert rows == completed_rows and outcome == dict(
        fixture_exit_code=101, responsiveness=responsiveness, baseline_responsiveness_failure=True)
    unobserved = json.loads(json.dumps(responsiveness))
    unobserved["conditions"]["guest_feedback"].update(observed=False, response_fragments=None)
    _, outcome = fixture_result(concurrent_output(unobserved), 101, CONCURRENT_TEST, "baseline")
    assert outcome["responsiveness"]["conditions"]["guest_feedback"]["response_fragments"] is None
    successful = json.loads(json.dumps(responsiveness))
    successful.update(ok=True, failures=[])
    successful["conditions"]["guest_feedback"].update(response_fragments=40, ok=True)
    _, outcome = fixture_result(concurrent_output(successful, failure=""), 0,
                                CONCURRENT_TEST, "candidate")
    assert outcome["responsiveness"]["ok"] is True
    assert outcome["baseline_responsiveness_failure"] is False

    def rejected(output, returncode=101, test=CONCURRENT_TEST, revision="baseline"):
        try:
            fixture_result(output, returncode, test, revision)
        except ValueError:
            return
        raise AssertionError("an incomplete or non-performance failure was accepted")

    rejected(concurrent_output(), revision="candidate")
    rejected(concurrent_output(), returncode=1)
    rejected(concurrent_output(), returncode=0)
    rejected(concurrent_output(), test=TEST)
    rejected(concurrent_output(successful, failure="thread 'worker' panicked at worker.rs:1:1:\nwrong"),
             returncode=0, revision="candidate")
    rejected(concurrent_output(failure="unrelated content assertion failed"))
    rejected(concurrent_output(failure=assertion + "\nthread 'worker' panicked at worker.rs:1:1:\nwrong"))
    rejected(concurrent_output(failure=assertion + "\n" + RESPONSIVENESS_FAILURE))
    rejected("\n".join(PREFIX + json.dumps(value) for value in completed_rows) + "\n" + assertion)
    rejected(concurrent_output() + "\n" + RESPONSIVENESS_PREFIX + json.dumps(responsiveness))
    rejected(RESPONSIVENESS_PREFIX + json.dumps(responsiveness) + "\n"
             + "\n".join(PREFIX + json.dumps(value) for value in completed_rows) + "\n" + assertion)
    rejected(concurrent_output(checkpoints=completed_rows[:-1]))
    incomplete_work = json.loads(json.dumps(completed_rows))
    incomplete_work[-1]["work"]["verified_complete_response"] = False
    rejected(concurrent_output(checkpoints=incomplete_work))
    for key, value in (("schema", 2), ("completed_fragments", 159), ("draft_verified", False),
                       ("source_copy_verified", False), ("cleanup_complete", False), ("failures", [])):
        invalid = dict(responsiveness, **{key: value})
        rejected(concurrent_output(invalid))
    malformed = json.loads(json.dumps(responsiveness))
    malformed["conditions"]["guest_feedback"]["observed"] = False
    rejected(concurrent_output(malformed))
    if Path("/proc").exists():
        with tempfile.TemporaryDirectory(prefix="maka-parity-cleanup-") as path:
            directory = Path(path)
            root = directory / "ui"
            other_root = directory / "unrelated"
            children = [subprocess.Popen([sys.executable, "-c", "import time; time.sleep(30)",
                                          "--root", str(candidate)], start_new_session=True)
                        for candidate in (root, other_root)]
            try:
                identity = process_identity(children[0].pid)
                assert identity is not None
                (directory / "owners.jsonl").write_text(json.dumps(dict(
                    ui_pid=children[0].pid, start_ticks=identity["start_ticks"],
                    ui_root=str(root), host_root=str(root / "host"), binary=sys.executable)) + "\n")
                cleaned = cleanup(directory, Path(sys.executable))
                children[0].wait(timeout=5)
                assert not cleaned["remaining"]
                assert children[1].poll() is None, "cleanup must preserve unrelated processes"
            finally:
                for child in children:
                    if child.poll() is None:
                        child.kill()
                    child.wait()
    print("frame comparison and independent PTY-owner cleanup checks passed")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--fixture", type=Path, help="compiled maka-cli integration test executable")
    parser.add_argument("--baseline", type=Path)
    parser.add_argument("--candidate", type=Path)
    parser.add_argument("--bundle", type=Path, help="one frozen checked product bundle for both debug CLIs")
    parser.add_argument("--output", type=Path)
    parser.add_argument("--runs", type=int, default=5)
    parser.add_argument("--case", choices=("parity", "concurrent"), default="parity")
    parser.add_argument("--timeout", type=float, default=240)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return
    if not all((args.fixture, args.baseline, args.candidate, args.output)) or args.runs < 1:
        parser.error("--fixture, --baseline, --candidate, --output and positive --runs are required")
    bundle = args.bundle or (Path(os.environ["MAKA_PRODUCT_BUNDLE"])
                             if os.environ.get("MAKA_PRODUCT_BUNDLE") else None)
    if bundle is None:
        parser.error("--bundle (or MAKA_PRODUCT_BUNDLE) must select the same checked product assets for both debug CLIs")
    bundle = bundle.resolve()
    try:
        product_version = (bundle / "product-version.txt").read_text().strip()
    except OSError as error:
        parser.error(f"product bundle is unavailable: {error}")
    if not Path("/proc").exists():
        parser.error("process-owner cleanup requires Linux /proc")
    args.output.mkdir(parents=True, exist_ok=False)
    test = CONCURRENT_TEST if args.case == "concurrent" else TEST
    evidence = dict(test=test, case=args.case, runs=args.runs, cpu_scope="Linux UI process and threads; excludes Host/fixture",
                    memory_scope="Linux UI process only: checkpoint VmRSS and process-wide VmHWM since startup, in bytes; not additive across checkpoints",
                    cpu_count=os.cpu_count(), load_average=os.getloadavg() if hasattr(os, "getloadavg") else None,
                    fixture=str(args.fixture.resolve()), baseline=str(args.baseline.resolve()),
                    candidate=str(args.candidate.resolve()),
                    product_bundle=dict(path=str(bundle), product_version=product_version),
                    samples=[], failures=[], baseline_responsiveness_failures=[])
    result_file = args.output / "results.json"
    def save():
        evidence["summary"] = summary(evidence["samples"])
        result_file.write_text(json.dumps(evidence, ensure_ascii=False, indent=2) + "\n")
    for pair in range(1, args.runs + 1):
        paired = {}
        for revision, binary in (("baseline", args.baseline), ("candidate", args.candidate)):
            directory = args.output / f"{pair:02d}-{revision}"
            directory.mkdir()
            environment = dict(os.environ, MAKA_TUI_PROBE_BINARY=str(binary.resolve()),
                               MAKA_PRODUCT_BUNDLE=str(bundle),
                               MAKA_TUI_PARITY_EVIDENCE=str(directory))
            started = time.monotonic()
            try:
                rows, outcome = run_fixture(args.fixture.resolve(), test, environment, directory,
                                            binary.resolve(), args.timeout, revision)
            except (OSError, ValueError, subprocess.TimeoutExpired) as error:
                evidence["failures"].append(dict(pair=pair, revision=revision, reason=str(error)))
                save()
                raise SystemExit(f"probe failed: {error}; evidence: {directory}") from error
            paired[revision] = rows
            evidence["samples"].append(dict(pair=pair, revision=revision, records=rows, outcome=outcome))
            if outcome["baseline_responsiveness_failure"]:
                evidence["baseline_responsiveness_failures"].append(dict(pair=pair, outcome=outcome))
            save()
            print(f"pair {pair}/{args.runs} {revision}: {len(rows)} checkpoints "
                  f"({time.monotonic() - started:.1f}s with setup)", flush=True)
        differences = args.output / f"{pair:02d}-comparison"
        differences.mkdir()
        errors = compare(paired["baseline"], paired["candidate"], differences)
        evidence["failures"].extend(dict(pair=pair, reason=error) for error in errors)
        save()
        if errors:
            raise SystemExit(f"UI parity failed: {errors}; inspect {differences}")
    baseline_failures = len(evidence["baseline_responsiveness_failures"])
    print(f"Complete-frame checkpoints match; baseline responsiveness failures: {baseline_failures}; "
          f"candidate strict fixture checks passed; measurements: {result_file}")


if __name__ == "__main__":
    main()
