"""Embedded pytest adapter. The private JSONL report is independent of console IO."""

import json
import os
import sys


def main():
    import pytest

    request_path, report_path = sys.argv[1:3]
    with open(request_path, encoding="utf-8") as stream:
        request = json.load(stream)
    sys.path.insert(0, os.getcwd())
    with open(report_path, "w", encoding="utf-8", buffering=1) as output:

        def emit(event, **values):
            output.write(
                json.dumps({"event": event, **values}, ensure_ascii=True) + "\n"
            )
            output.flush()

        class Bridge:
            def __init__(self):
                self.cases = {}

            def pytest_configure(self, config):
                self.root = str(config.rootpath)

            def pytest_collection_finish(self, session):
                items = session.items
                if len(items) > 10000:
                    raise pytest.UsageError(
                        "Helix supports at most 10000 cases per run"
                    )
                for item in items:
                    _, line, _ = item.location
                    path = str(item.path.resolve())
                    tail = item.nodeid.partition("::")[2]
                    case = dict(
                        id=item.nodeid,
                        name=item.name,
                        selector=path + ("::" + tail if tail else ""),
                        path=path,
                        line=line,
                        outcome="queued",
                        duration=0.0,
                        details="",
                    )
                    self.cases[item.nodeid] = case
                    emit("collected", case=case)

            def pytest_runtest_logstart(self, nodeid, location):
                emit("started", id=nodeid)

            def pytest_runtest_logreport(self, report):
                case = self.cases.get(report.nodeid)
                if case is None:
                    return
                case["duration"] += report.duration
                # A successful call is provisional until teardown has completed.
                if report.failed:
                    case["outcome"] = "failed" if report.when == "call" else "error"
                    case["details"] += str(report.longrepr)[-16000:] + "\n"
                    crash = getattr(report.longrepr, "reprcrash", None)
                    if crash:
                        path = str(crash.path)
                        if not os.path.isabs(path) and not os.path.isfile(path):
                            path = os.path.join(self.root, path)
                        case["failure_path"] = os.path.abspath(path)
                        case["failure_line"] = max(0, crash.lineno - 1)
                elif report.skipped and case["outcome"] not in ("failed", "error"):
                    case["outcome"] = "skipped"
                    case["details"] += str(report.longrepr)[-4000:] + "\n"
                if report.when == "teardown":
                    if case["outcome"] == "queued":
                        case["outcome"] = "passed"
                    sections = dict(report.sections)
                    for title, value in sections.items():
                        case["details"] += "\n" + title + "\n" + value[-8000:]
                    case["details"] = case["details"][-32000:]
                    emit("result", case=case)

        code = pytest.main(["--color=no", "-q", *request["args"]], plugins=[Bridge()])
        emit("finished", code=int(code))
    raise SystemExit(code)


if __name__ == "__main__":
    main()
