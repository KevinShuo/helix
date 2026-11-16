"""Process regressions for the real hx AI manager; uses synthetic models, no GPU.

Run: python contrib/test_shared_ai.py --hx target/release/hx.exe
"""

import argparse
import json
import os
from pathlib import Path
import socket
import struct
import subprocess
import tempfile
import time
import unittest
import urllib.request


PARSER = argparse.ArgumentParser()
PARSER.add_argument("--hx", type=Path, required=True)
OPTIONS, TEST_ARGS = PARSER.parse_known_args()
HX = OPTIONS.hx.resolve()
FLAGS = subprocess.CREATE_NO_WINDOW if os.name == "nt" else 0


def wait_for(predicate, timeout=15):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = predicate()
        if result:
            return result
        time.sleep(0.03)
    raise AssertionError("timed out waiting for condition")


def port():
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        return listener.getsockname()[1]


def send(stream, value):
    body = json.dumps(value).encode()
    stream.sendall(struct.pack(">I", len(body)) + body)


def receive(stream):
    def read_exact(n):
        result = b""
        while len(result) < n:
            chunk = stream.recv(n - len(result))
            if not chunk:
                raise EOFError("manager disconnected")
            result += chunk
        return result

    return json.loads(read_exact(struct.unpack(">I", read_exact(4))[0]))


class SharedAI(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.build = tempfile.TemporaryDirectory(prefix="hx-shared-ai-build-")
        cls.stub = Path(cls.build.name) / ("server.exe" if os.name == "nt" else "server")
        fixture = Path(__file__).resolve().parents[1] / "helix-term/tests/fixtures/ai-model-server.rs"
        subprocess.run(["rustc", "--edition=2021", str(fixture), "-o", str(cls.stub)], check=True,
                       capture_output=True, creationflags=FLAGS)

    @classmethod
    def tearDownClass(cls):
        cls.build.cleanup()

    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="hx-shared-ai-")
        self.root = Path(self.temp.name)
        self.manager = self.root / "manager"
        self.processes = []
        self.streams = []
        self.baseline = self.model("baseline-9b")
        self.temporary = self.model("temporary-27b", peer=self.baseline)

    def tearDown(self):
        for stream in self.streams:
            stream.close()
        for process in self.processes:
            if process.poll() is None:
                process.kill()
            process.wait(timeout=5)
            if process.stderr is not None:
                process.stderr.close()
        self.temp.cleanup()

    def model(self, name, delay=0, peer=None):
        model_path = self.root / (name + ".gguf")
        peer_port = int(peer["endpoint"].split(":")[2].split("/")[0]) if peer else 0
        model_path.write_text(f"peer={peer_port}\ndelay={delay}\n")
        return {"type": "llama-cpp", "endpoint": f"http://127.0.0.1:{port()}/v1/chat/completions",
                "model": name, "model-path": str(model_path), "server-path": str(self.stub),
                "context-size": 4096, "gpu-layers": 0, "startup-timeout-ms": 10000}

    def start(self):
        process = subprocess.Popen([str(HX), "--internal-ai-manager", str(self.manager)],
                                   stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                                   stderr=subprocess.PIPE, creationflags=FLAGS)
        self.processes.append(process)

        def discovery():
            if process.poll() is not None:
                self.fail("hx manager exited before discovery: " + process.stderr.read().decode(errors="replace"))
            try:
                info = json.loads((self.manager / "manager.json").read_text())
                return info if info["pid"] == process.pid else None
            except (FileNotFoundError, json.JSONDecodeError):
                return None

        self.info = wait_for(discovery)
        return process

    def connect(self, request, token=None):
        stream = socket.create_connection(("127.0.0.1", self.info["port"]), timeout=15)
        self.streams.append(stream)
        send(stream, {"version": 1, "token": token or self.info["token"], "request": request})
        return stream

    def register(self):
        stream = self.connect({"type": "register", "resident": self.baseline})
        self.assertEqual(receive(stream), {"status": "ok"})
        return stream

    def acquire(self, model=None, restore=None):
        stream = self.connect({"type": "acquire", "model": model or self.baseline, "restore": restore})
        self.assertEqual(receive(stream), {"status": "ok"})
        return stream

    def finish(self, stream):
        stream.sendall(b"\x01")
        self.assertEqual(receive(stream), {"status": "ok"})
        stream.close()

    def reachable(self, model):
        try:
            with urllib.request.urlopen(model["endpoint"].replace("chat/completions", "models"), timeout=.2) as response:
                return response.status == 200
        except OSError:
            return False

    def starts(self, model):
        path = Path(model["model-path"]).with_suffix(".started")
        return len(path.read_text().splitlines()) if path.exists() else 0

    def test_first_editor_exit_keeps_shared_model_until_last_editor_exits(self):
        manager = self.start()
        first, second = self.register(), self.register()
        self.finish(self.acquire())
        self.assertEqual(self.starts(self.baseline), 1, "both editors must share one model")
        first.close()
        self.finish(self.acquire())
        self.assertTrue(self.reachable(self.baseline))
        self.assertEqual(self.starts(self.baseline), 1, "first editor exit must not restart the model")
        second.close()
        manager.wait(timeout=14)
        self.assertEqual(manager.returncode, 0)
        self.assertFalse(self.reachable(self.baseline), "last editor exit must release the model")

    def test_temporary_lease_serializes_other_editors_and_restores_completion(self):
        self.start()
        self.register()
        self.register()
        self.finish(self.acquire())
        temporary = self.acquire(self.temporary, self.baseline)
        self.assertFalse(self.reachable(self.baseline))
        queued = self.connect({"type": "acquire", "model": self.baseline, "restore": None})
        queued.settimeout(.2)
        with self.assertRaises(socket.timeout):
            queued.recv(1)
        self.finish(temporary)
        queued.settimeout(10)
        self.assertEqual(receive(queued), {"status": "ok"})
        self.assertTrue(self.reachable(self.baseline))
        self.assertFalse(self.reachable(self.temporary))
        self.finish(queued)
        self.finish(self.acquire(self.temporary, self.baseline))
        self.finish(self.acquire())

    def test_disconnected_loading_and_queued_requests_do_not_block_other_editors(self):
        self.start()
        self.register()
        self.finish(self.acquire())
        slow = self.model("slow-27b", delay=2000, peer=self.baseline)
        pending = self.connect({"type": "acquire", "model": slow, "restore": self.baseline})
        wait_for(lambda: self.starts(slow))
        queued = self.connect({"type": "acquire", "model": self.temporary, "restore": self.baseline})
        queued.close()
        pending.close()
        self.finish(self.acquire())
        self.assertFalse(self.reachable(slow))
        self.assertEqual(self.starts(self.temporary), 0, "disconnected queued request must never load")

    def test_simultaneous_start_does_not_replace_live_manager_credentials(self):
        manager = self.start()
        self.register()
        info = self.info.copy()
        contenders = [subprocess.Popen([str(HX), "--internal-ai-manager", str(self.manager)],
                                      stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, creationflags=FLAGS)
                      for _ in range(3)]
        self.processes.extend(contenders)
        for contender in contenders:
            self.assertEqual(contender.wait(timeout=5), 0)
        self.assertIsNone(manager.poll())
        self.assertEqual(json.loads((self.manager / "manager.json").read_text()), info)
        self.finish(self.acquire())

    def test_authentication_and_oversized_frames_cannot_load_models(self):
        self.start()
        stream = self.connect({"type": "register", "resident": self.baseline}, token="wrong")
        self.assertEqual(receive(stream)["status"], "error")
        stream = socket.create_connection(("127.0.0.1", self.info["port"]), timeout=2)
        self.streams.append(stream)
        stream.sendall(struct.pack(">I", 65537))
        try:
            self.assertFalse(stream.recv(1))
        except ConnectionResetError:
            pass
        self.assertEqual(self.starts(self.baseline), 0)

    def test_manager_crash_releases_models_and_stale_discovery_can_restart(self):
        manager = self.start()
        self.register()
        self.finish(self.acquire())
        manager.kill()
        manager.wait(timeout=5)
        wait_for(lambda: not self.reachable(self.baseline))
        self.start()
        self.register()
        self.finish(self.acquire())
        self.assertEqual(self.starts(self.baseline), 2)

    def test_incompatible_runtime_reloads_model_instead_of_alias_only_reuse(self):
        self.start()
        self.register()
        self.finish(self.acquire())
        updated = dict(self.baseline, runtime={"threads": 3})
        self.finish(self.acquire(updated))
        self.assertEqual(self.starts(self.baseline), 2)
        arguments = Path(self.baseline["model-path"]).with_suffix(".args").read_text().splitlines()
        self.assertIn(["--threads", "3"], [arguments[i:i + 2] for i in range(len(arguments) - 1)])

    def test_unknown_external_model_is_reused_but_never_killed_to_switch(self):
        baseline_port = self.baseline["endpoint"].split(":")[2].split("/")[0]
        external = subprocess.Popen([str(self.stub), "--model", self.baseline["model-path"],
                                     "--alias", self.baseline["model"], "--port", baseline_port],
                                    stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, creationflags=FLAGS)
        self.processes.append(external)
        wait_for(lambda: self.reachable(self.baseline))
        self.start()
        self.register()
        self.finish(self.acquire())
        denied = self.connect({"type": "acquire", "model": self.temporary, "restore": self.baseline})
        response = receive(denied)
        self.assertEqual(response["status"], "error")
        self.assertIn("externally owned", response["message"])
        self.assertIsNone(external.poll())
        self.assertTrue(self.reachable(self.baseline))
        self.assertEqual(self.starts(self.temporary), 0)

    def test_disconnected_inference_restores_model_for_other_editor(self):
        self.start()
        self.register()
        self.register()
        self.finish(self.acquire())
        active = self.acquire(self.temporary, self.baseline)
        active.close()
        self.finish(self.acquire())
        self.assertFalse(self.reachable(self.temporary))
        self.assertTrue(self.reachable(self.baseline))

    def test_last_editor_exit_cancels_slow_restoration_within_idle_deadline(self):
        manager = self.start()
        editor = self.register()
        self.finish(self.acquire())
        active = self.acquire(self.temporary, self.baseline)
        Path(self.baseline["model-path"]).write_text("peer=0\ndelay=20000\n")
        editor.close()
        active.close()
        manager.wait(timeout=14)
        self.assertEqual(manager.returncode, 0)
        self.assertFalse(self.reachable(self.baseline))
        self.assertFalse(self.reachable(self.temporary))

    def test_exited_external_owner_can_be_replaced_without_restarting_editors(self):
        baseline_port = self.baseline["endpoint"].split(":")[2].split("/")[0]
        external = subprocess.Popen([str(self.stub), "--model", self.baseline["model-path"],
                                     "--alias", self.baseline["model"], "--port", baseline_port],
                                    stdout=subprocess.DEVNULL, stderr=subprocess.PIPE, creationflags=FLAGS)
        self.processes.append(external)
        wait_for(lambda: self.reachable(self.baseline))
        self.start()
        self.register()
        self.finish(self.acquire())
        external.kill()
        external.wait(timeout=5)
        self.finish(self.acquire())
        self.assertEqual(self.starts(self.baseline), 2)
        self.finish(self.acquire(self.temporary, self.baseline))


if __name__ == "__main__":
    unittest.main(argv=[__file__] + TEST_ARGS, verbosity=2)
