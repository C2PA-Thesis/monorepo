"""Exercise the local web entry point with real proof tools and isolated files."""
from __future__ import annotations

import json
import os
import socket
import subprocess
import threading
import time
import uuid
from pathlib import Path

import httpx
from PIL import Image

ROOT = Path(__file__).resolve().parents[2]
OUTPUT = ROOT / "web/generated/integration" / uuid.uuid4().hex


def main():
    OUTPUT.mkdir(parents=True)
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        port = listener.getsockname()[1]
    env = dict(os.environ, WEB_SKIP_SETUP="1", LAB_PORT=str(port), LAB_RUNS_DIR=str(OUTPUT / "runs"))
    events = []
    stream_errors = []
    with (OUTPUT / "server.log").open("w") as log:
        process = subprocess.Popen([str(ROOT / "web/run.sh")], cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT)
        try:
            with httpx.Client(base_url="http://127.0.0.1:{}".format(port), timeout=30) as client:
                for _ in range(100):
                    try:
                        ready = client.get("/api/health")
                        ready.raise_for_status()
                        assert ready.json()["ready"], ready.text
                        break
                    except httpx.ConnectError:
                        assert process.poll() is None, "Web server exited; inspect " + str(OUTPUT)
                        time.sleep(.1)
                else:
                    raise AssertionError("Web server did not become ready")
                assert "From pixels to proof" in client.get("/").text
                photo = ROOT / "fixtures/generated/demo-input.jpg"
                response = client.post("/api/runs", files={"file": ("integration.jpg", photo.read_bytes(), "image/jpeg")})
                response.raise_for_status()
                identifier = response.json()["id"]
                path = "/api/runs/" + identifier

                def receive_events():
                    try:
                        with httpx.stream("GET", str(client.base_url).rstrip("/") + path + "/events", timeout=30) as stream:
                            stream.raise_for_status()
                            for line in stream.iter_lines():
                                if line.startswith("data: "):
                                    event = json.loads(line[6:])
                                    events.append(event)
                                    if event["type"] == "state" and event.get("status") in {"complete", "failed", "cancelled"}:
                                        return
                    except Exception as error:
                        stream_errors.append(str(error))

                listener = threading.Thread(target=receive_events, daemon=True)
                listener.start()
                location = {"latitude": 37.7749, "longitude": -122.4194, "resolution": 7}
                region = client.post("/api/regions", json=location).json()
                assert region["supported"] and region["contains_point"], region
                response = client.post(path + "/start", json=location)
                response.raise_for_status()
                assert client.post(path + "/start", json=location).status_code == 409
                deadline = time.monotonic() + 900
                while time.monotonic() < deadline:
                    run = client.get(path).json()
                    if run["status"] not in {"queued", "running"}:
                        break
                    time.sleep(1)
                assert run["status"] == "complete", run
                assert len(run["stages"]) == 8 and all(stage["status"] == "complete" for stage in run["stages"])
                listener.join(timeout=5)
                assert not listener.is_alive(), "Event stream never reported completion"
                assert not stream_errors, stream_errors
                assert sum(e["type"] == "stage" and e["status"] == "complete" for e in events) == 8
                assert any(e["type"] == "message" for e in events)
                reader = client.get(path + "/reader").json()
                assert reader["published"] and reader["assertion"]["region"]["cell"] == region["cell"]
                assert reader["manifest"]["validation_state"] == "Valid", reader["manifest"]
                assert len(reader["negative_results"]["cases"]) == 6
                for case in reader["negative_results"]["cases"]:
                    assert case["observed"] in case["expected"].split(" or "), case
                    assert case["c2pa_validation_state"] == ("not applicable" if case["observed"] == "prove refused" else "Valid"), case
                for private in ["config", "latitude", "longitude", "source", "directory"]:
                    assert private not in reader
                signed = client.get(path + "/assets/signed.png?download=true")
                signed.raise_for_status()
                (OUTPUT / "signed.png").write_bytes(signed.content)
                directory = OUTPUT / "runs" / identifier
                with Image.open(directory / "prepared.png") as original, Image.open(OUTPUT / "signed.png") as published:
                    assert published.size == (512, 512)
                    assert original.crop((0, 0, 512, 512)).tobytes() == published.convert("RGB").tobytes()
                assert client.get(path + "/assets/secrets.json").status_code == 404
                (OUTPUT / "result.json").write_text(json.dumps({"run": run, "reader": reader, "events": events}, indent=2))
                print("PASS: real upload, region preflight, frozen capture, eight stages, SSE, signed pixels, reader evidence and six negative cases")
                print("Evidence: " + str(OUTPUT))
        finally:
            process.terminate()
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                process.kill()
                process.wait()


if __name__ == "__main__":
    main()
