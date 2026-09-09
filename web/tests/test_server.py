from __future__ import annotations

import io
import json
import os
import sys
import tempfile
import threading
import time
import unittest
from pathlib import Path
from unittest import mock

import h3
from fastapi.testclient import TestClient
from PIL import Image

import server
from lib.workflow import Stage, build_stages


class LabTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.lab = server.Lab(Path(self.temporary.name))
        self.patch = mock.patch.object(server, "lab", self.lab)
        self.patch.start()
        self.client = TestClient(server.app)
        image = Image.new("RGB", (40, 20), (120, 80, 40))
        buffer = io.BytesIO()
        image.save(buffer, "PNG")
        self.image = buffer.getvalue()

    def tearDown(self):
        self.lab.close()
        self.client.close()
        self.patch.stop()
        self.temporary.cleanup()

    def upload(self):
        result = self.client.post("/api/runs", files={"file": ("my photo.png", self.image, "image/png")})
        self.assertEqual(result.status_code, 200, result.text)
        return result.json()

    def test_upload_creates_isolated_normalized_pixels_and_exact_left_crop(self):
        first, second = self.upload(), self.upload()
        self.assertNotEqual(first["id"], second["id"])
        self.assertEqual(first["source"]["width"], 40)
        self.assertEqual(first["status"], "draft")
        directory = self.lab.get(first["id"]).directory
        with Image.open(directory / "prepared.png") as original, Image.open(directory / "crop-preview.png") as crop:
            self.assertEqual(original.size, (1024, 512))
            self.assertEqual(crop.size, (512, 512))
            self.assertEqual(original.crop((0, 0, 512, 512)).tobytes(), crop.tobytes())
        matrix = self.client.get("/api/runs/{}/matrix?channel=G&x=16&y=8".format(first["id"])).json()
        self.assertEqual(matrix["values"], [80] * 64)
        self.assertEqual(matrix["indices"][0], 8 * 1024 + 16)
        self.assertEqual(matrix["indices"][8], 9 * 1024 + 16)

    def test_invalid_upload_and_cross_origin_request_are_rejected(self):
        result = self.client.post("/api/runs", files={"file": ("bad.png", b"not a photo", "image/png")})
        self.assertEqual(result.status_code, 422)
        self.assertEqual(len(self.lab.jobs), 0)
        result = self.client.post("/api/runs/sample", headers={"origin": "https://unrelated.example"})
        self.assertEqual(result.status_code, 403)

    def test_private_secrets_and_arbitrary_files_are_not_downloadable(self):
        item = self.upload()
        for filename in ["secrets.json", "device-private.pem", "run.json", "prepared.json"]:
            response = self.client.get("/api/runs/{}/assets/{}".format(item["id"], filename))
            self.assertEqual(response.status_code, 404)
        reader = self.client.get("/api/runs/{}/reader".format(item["id"])).json()
        self.assertFalse(reader["published"])
        for private in ["config", "source", "latitude", "longitude", "directory"]:
            self.assertNotIn(private, reader)

    def test_shared_plan_uses_the_selected_location_and_isolated_output(self):
        stages = build_stages(Path("photo.png"), 35.6762, 139.6503, Path("tokyo.json"), Path("isolated/out"), {"HV_BIN": "hv", "ZKLOC_BIN": "zkloc"})
        self.assertEqual(len(stages), 8)
        command = [str(value) for value in stages[0].command]
        self.assertEqual(command[command.index("--lat") + 1], "35.6762")
        self.assertEqual(command[command.index("--lon") + 1], "139.6503")
        self.assertIn("isolated/out/capture", command)
        self.assertIn(Path("tokyo.json"), stages[2].command)

    def test_failed_command_never_marks_the_run_complete(self):
        item = self.upload()
        job = self.lab.get(item["id"])
        job.update(stages=[{"id": "broken", "status": "pending"}])
        stage = Stage(1, "broken", "Broken command", "", [sys.executable, "-c", "import sys; print('actual failure'); sys.exit(7)"], [])
        self.lab.run(job, [stage], os.environ.copy())
        self.assertEqual(job.data["status"], "failed")
        self.assertEqual(job.data["stages"][0]["status"], "failed")
        self.assertIn("actual failure", job.data["error"])
        self.assertIn("actual failure", (job.directory / "logs/broken.log").read_text())

    def test_cancel_stops_a_running_process_and_preserves_its_files(self):
        item = self.upload()
        job = self.lab.get(item["id"])
        job.update(stages=[{"id": "wait", "status": "pending"}])
        stage = Stage(1, "wait", "Long command", "", [sys.executable, "-c", "import time; print('::poc:: Ready', flush=True); time.sleep(30)"], [])
        thread = threading.Thread(target=self.lab.run, args=(job, [stage], os.environ.copy()))
        thread.start()
        deadline = time.monotonic() + 5
        while job.process is None and time.monotonic() < deadline:
            time.sleep(.01)
        self.assertIsNotNone(job.process)
        result = self.client.post("/api/runs/{}/cancel".format(item["id"]))
        self.assertEqual(result.status_code, 200)
        thread.join(timeout=6)
        self.assertFalse(thread.is_alive())
        self.assertEqual(job.data["status"], "cancelled")
        self.assertTrue((job.directory / "prepared.png").is_file())

    def test_restart_retains_completed_and_marks_interrupted_runs(self):
        item = self.upload()
        job = self.lab.get(item["id"])
        job.update(status="running")
        recovered = server.Lab(self.lab.root)
        try:
            self.assertEqual(recovered.get(item["id"]).data["status"], "failed")
            self.assertIn("lab stopped", recovered.get(item["id"]).data["error"])
        finally:
            recovered.close()

    @unittest.skipUnless((server.GENERATED / "bin/zkloc").is_file(), "Requires the real location CLI")
    def test_real_region_preflight_accepts_utdt_and_rejects_pentagons(self):
        accepted = self.client.post("/api/regions", json={"latitude": -34.5478, "longitude": -58.4462, "resolution": 7}).json()
        self.assertTrue(accepted["supported"])
        self.assertTrue(accepted["contains_point"])
        pentagon = h3.get_pentagons(7)[0]
        lat, lng = h3.cell_to_latlng(pentagon)
        rejected = self.client.post("/api/regions", json={"latitude": lat, "longitude": lng, "resolution": 7, "cell": pentagon}).json()
        self.assertFalse(rejected["supported"])
        self.assertIn("pentagon", rejected["reason"])


if __name__ == "__main__":
    unittest.main()
