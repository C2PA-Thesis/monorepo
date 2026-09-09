from __future__ import annotations

import contextlib
import io
import os
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path
from unittest import mock

import demo
from lib.presentation import Console, StepFailed, announce


class DemoTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="poc display ")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)

    def test_plain_display_explains_events_and_keeps_full_logs(self):
        output = io.StringIO()
        log = self.root / "stage.log"
        with contextlib.redirect_stdout(output):
            elapsed = Console(plain=True).run(
                "Small command",
                [sys.executable, "-c", "print('::poc:: Read the photo.'); print('raw detail')"],
                log, os.environ.copy(), self.root,
            )
        self.assertGreater(elapsed, 0)
        self.assertIn("-> Read the photo.", output.getvalue())
        self.assertIn("[OK]", output.getvalue())
        self.assertNotIn("raw detail", output.getvalue())
        self.assertNotIn("\x1b", output.getvalue())
        self.assertIn("Command:", log.read_text())
        self.assertIn("raw detail", log.read_text())

    def test_failed_child_preserves_exit_status_and_shows_its_error(self):
        output = io.StringIO()
        with contextlib.redirect_stdout(output), self.assertRaises(StepFailed) as failure:
            Console(plain=True).run(
                "Broken command",
                [sys.executable, "-c", "import sys; print('cannot read photo', file=sys.stderr); sys.exit(7)"],
                self.root / "failed.log", os.environ.copy(), self.root,
            )
        self.assertEqual(failure.exception.returncode, 7)
        self.assertIn("[FAILED] Broken command", output.getvalue())
        self.assertIn("cannot read photo", output.getvalue())

    def test_image_path_with_spaces_is_resolved_without_a_shell(self):
        from PIL import Image

        photo = self.root / "my photo $(do not execute).png"
        Image.new("RGB", (20, 10), "red").save(photo)
        output = io.StringIO()
        with mock.patch.dict(os.environ, DEMO_PHOTO=str(photo)), contextlib.redirect_stdout(output):
            selected = demo.selected_photo()
            Console(plain=True).preview(selected, Path(sys.executable), "Selected photo")
        self.assertEqual(selected, photo.resolve())
        self.assertIn(str(photo.resolve()), output.getvalue())
        self.assertIn("PNG | 20 x 10 pixels", output.getvalue())

    def test_missing_or_output_directory_input_stops_before_running_commands(self):
        out = self.root / "out"
        out.mkdir()
        sentinel = out / "previous-result.png"
        sentinel.write_bytes(b"keep the previous run")
        for photo in [sentinel, self.root / "missing.jpg"]:
            with self.subTest(photo=photo), mock.patch.object(demo, "OUT", out), mock.patch.dict(
                os.environ, DEMO_PHOTO=str(photo)
            ), mock.patch.object(Console, "run") as command:
                with self.assertRaises(ValueError):
                    demo.run(Console(plain=True))
                command.assert_not_called()
                self.assertEqual(sentinel.read_bytes(), b"keep the previous run")

    def test_progress_messages_are_opt_in(self):
        output = io.StringIO()
        with contextlib.redirect_stdout(output), mock.patch.dict(os.environ, PIPELINE_EXPLAIN="0"):
            announce("quiet")
        self.assertEqual(output.getvalue(), "")
        with contextlib.redirect_stdout(output), mock.patch.dict(os.environ, PIPELINE_EXPLAIN="1"):
            announce("Read the photo.")
        self.assertEqual(output.getvalue(), "::poc:: Read the photo.\n")

    @unittest.skipUnless(os.name == "posix", "The shell pipeline runs on POSIX systems")
    def test_ctrl_c_stops_the_active_child_and_grandchild(self):
        child_ready = self.root / "child-ready"
        stopped = self.root / "grandchild-stopped"
        grandchild = self.root / "grandchild.py"
        grandchild.write_text(
            "import signal, sys, time\n"
            "from pathlib import Path\n"
            "def stop(*args):\n"
            "    Path(sys.argv[2]).touch()\n"
            "    raise SystemExit(0)\n"
            "signal.signal(signal.SIGTERM, stop)\n"
            "Path(sys.argv[1]).touch()\n"
            "time.sleep(30)\n"
        )
        child = self.root / "child.py"
        child.write_text(
            "import subprocess, sys, time\n"
            "subprocess.Popen([sys.executable] + sys.argv[1:])\n"
            "time.sleep(30)\n"
        )
        runner = self.root / "runner.py"
        runner.write_text(
            "import os, sys\n"
            "from pathlib import Path\n"
            "from lib.presentation import Console\n"
            "try:\n"
            "    Console(plain=True).run('Waiting', [sys.executable] + sys.argv[1:], "
            "Path('cancel.log'), os.environ.copy(), Path.cwd())\n"
            "except KeyboardInterrupt:\n"
            "    raise SystemExit(130)\n"
        )
        process = subprocess.Popen(
            [sys.executable, str(runner), str(child), str(grandchild), str(child_ready), str(stopped)],
            cwd=self.root,
            env=dict(os.environ, PYTHONPATH=str(demo.PIPELINE)),
            stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True,
        )
        try:
            deadline = time.monotonic() + 5
            while not child_ready.exists() and process.poll() is None and time.monotonic() < deadline:
                time.sleep(0.02)
            self.assertTrue(child_ready.exists(), "The test grandchild did not start")
            process.send_signal(signal.SIGINT)
            stdout, stderr = process.communicate(timeout=8)
            self.assertEqual(process.returncode, 130, stdout + stderr)
            self.assertTrue(stopped.exists(), "Ctrl+C left the grandchild running")
        finally:
            if process.poll() is None:
                process.send_signal(signal.SIGINT)
                process.communicate(timeout=8)


if __name__ == "__main__":
    unittest.main()
