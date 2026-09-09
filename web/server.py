from __future__ import annotations

import asyncio
import base64
import io
import json
import math
import os
import queue
import re
import signal
import struct
import subprocess
import threading
import time
import uuid
import warnings
from concurrent.futures import ThreadPoolExecutor
from contextlib import asynccontextmanager
from datetime import datetime, timezone
from pathlib import Path
from typing import Optional
from urllib.parse import urlparse

import h3
from fastapi import FastAPI, File, HTTPException, Request, UploadFile
from fastapi.responses import FileResponse, JSONResponse, StreamingResponse
from fastapi.staticfiles import StaticFiles
from fastapi.middleware.trustedhost import TrustedHostMiddleware
from PIL import Image
from pydantic import BaseModel, Field

from lib.images import canonicalize_capture, crop_left_half
from lib.c2pa import assertion_from_manifest
from lib.presentation import EVENT_PREFIX, clean
from lib.workflow import ROOT, PIPELINE, GENERATED, build_stages, runtime_env


WEB = ROOT / "web"
RUNS = Path(os.environ.get("LAB_RUNS_DIR", str(WEB / "generated/runs")))
MAX_UPLOAD = 20 * 1024 * 1024
TERMINAL = {"complete", "failed", "cancelled", "draft"}


def now():
    return datetime.now(timezone.utc).isoformat()


def read_json(path):
    try:
        return json.loads(path.read_text())
    except (FileNotFoundError, json.JSONDecodeError):
        return None


def write_json(path, data):
    temporary = path.with_suffix(".tmp")
    temporary.write_text(json.dumps(data, indent=2, allow_nan=False))
    temporary.replace(path)


class Location(BaseModel):
    latitude: float = Field(default=-34.5478, ge=-90, le=90)
    longitude: float = Field(default=-58.4462, ge=-180, le=180)
    resolution: int = Field(default=7, ge=0, le=15)
    cell: Optional[str] = Field(default=None, max_length=16)


def describe_region(location: Location):
    latitude = struct.unpack("f", struct.pack("f", math.radians(location.latitude)))[0]
    longitude = struct.unpack("f", struct.pack("f", math.radians(location.longitude)))[0]
    containing_cell = h3.latlng_to_cell(math.degrees(latitude), math.degrees(longitude), location.resolution)
    cell = location.cell or containing_cell
    if not h3.is_valid_cell(cell):
        raise HTTPException(422, "Enter a valid H3 cell.")
    result = {
        "cell": cell, "resolution": h3.get_resolution(cell),
        "boundary": h3.cell_to_boundary(cell), "center": h3.cell_to_latlng(cell),
        "area_km2": h3.cell_area(cell, unit="km^2"),
        "contains_point": cell == containing_cell, "supported": False, "reason": None,
    }
    import tempfile
    with tempfile.TemporaryDirectory(prefix="lab-region-") as temporary:
        out = Path(temporary) / "region.json"
        try:
            completed = subprocess.run(
                [os.environ.get("ZKLOC_BIN", str(GENERATED / "bin/zkloc")), "region", "--cell", cell, "--out", str(out)],
                capture_output=True, text=True, timeout=15,
            )
        except (OSError, subprocess.TimeoutExpired):
            result["reason"] = "The location tool is unavailable. Run ./pipeline/setup.sh."
            return result
        if completed.returncode:
            result["reason"] = clean(completed.stderr.strip()).removeprefix("zkloc: ")
        else:
            result.update(supported=True, region=read_json(out))
    return result


class Cancelled(Exception):
    pass


class Experiment:
    def __init__(self, directory, data):
        self.directory = directory
        self.data = data
        self.lock = threading.RLock()
        self.cancel = threading.Event()
        self.process = None
        self.events = []
        log = directory / "events.jsonl"
        if log.is_file():
            for line in log.read_text().splitlines():
                try:
                    self.events.append(json.loads(line))
                except json.JSONDecodeError:
                    continue

    def update(self, **values):
        with self.lock:
            self.data.update(values)
            write_json(self.directory / "run.json", self.data)

    def emit(self, kind, **values):
        with self.lock:
            event = {"id": len(self.events) + 1, "time": now(), "type": kind, **values}
            self.events.append(event)
            with (self.directory / "events.jsonl").open("a") as file:
                file.write(json.dumps(event) + "\n")
            return event

    def snapshot(self):
        with self.lock:
            return json.loads(json.dumps(self.data))

    def stop_process(self):
        process = self.process
        if process is not None:
            try:
                os.killpg(process.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass

    def execute(self, label, command, env):
        if self.cancel.is_set():
            raise Cancelled()
        logs = self.directory / "logs"
        logs.mkdir(exist_ok=True)
        started = time.monotonic()
        lines = queue.Queue()
        tail = []
        with (logs / (label + ".log")).open("w") as log:
            log.write("Command: " + json.dumps([str(value) for value in command]) + "\n")
            process = subprocess.Popen(
                [str(value) for value in command], cwd=ROOT, env=env,
                stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                text=True, encoding="utf-8", errors="replace", start_new_session=True,
            )
            self.process = process

            def read_output():
                for line in process.stdout:
                    lines.put(line)
                lines.put(None)

            reader = threading.Thread(target=read_output, daemon=True)
            reader.start()
            finished = False
            stop_at = None
            try:
                while not (finished and process.poll() is not None):
                    if self.cancel.is_set():
                        if stop_at is None:
                            self.stop_process()
                            stop_at = time.monotonic()
                        elif time.monotonic() - stop_at > 2:
                            try:
                                os.killpg(process.pid, signal.SIGKILL)
                            except ProcessLookupError:
                                pass
                    try:
                        line = lines.get(timeout=0.1)
                    except queue.Empty:
                        continue
                    if line is None:
                        finished = True
                        continue
                    log.write(line)
                    log.flush()
                    message = clean(line).strip()
                    tail = (tail + [message])[-8:]
                    if message.startswith(EVENT_PREFIX):
                        self.emit("message", stage=label, message=message[len(EVENT_PREFIX):])
                code = process.wait()
            finally:
                self.process = None
                reader.join(timeout=2)
                process.stdout.close()
        if self.cancel.is_set():
            raise Cancelled()
        if code:
            raise RuntimeError("\n".join(tail) or "Command failed with exit {}".format(code))
        return time.monotonic() - started


class Lab:
    def __init__(self, root):
        self.root = root
        self.root.mkdir(parents=True, exist_ok=True)
        self.jobs = {}
        self.lock = threading.RLock()
        self.worker = ThreadPoolExecutor(max_workers=1, thread_name_prefix="proof-lab")
        for directory in self.root.iterdir():
            if not re.fullmatch(r"[0-9a-f]{32}", directory.name):
                continue
            data = read_json(directory / "run.json")
            if data:
                job = Experiment(directory, data)
                if data["status"] not in TERMINAL:
                    job.update(status="failed", error="The lab stopped during this operation. Start a new experiment to retry.")
                self.jobs[directory.name] = job

    def get(self, identifier):
        with self.lock:
            if identifier not in self.jobs:
                raise HTTPException(404, "Experiment not found.")
            return self.jobs[identifier]

    def create(self, content, name):
        if len(content) > MAX_UPLOAD:
            raise HTTPException(413, "Choose an image smaller than 20 MB.")
        try:
            with warnings.catch_warnings():
                warnings.simplefilter("error", Image.DecompressionBombWarning)
                with Image.open(io.BytesIO(content)) as image:
                    if image.width * image.height > 40_000_000:
                        raise ValueError("Choose an image with fewer than 40 million pixels.")
                    source = {"width": image.width, "height": image.height, "format": image.format, "bytes": len(content)}
                    rgb = image.convert("RGB")
                    rgb.load()
        except Exception as error:
            raise HTTPException(422, "Cannot read this image. Use a valid PNG, JPEG, or WebP. " + str(error))
        identifier = uuid.uuid4().hex
        directory = self.root / identifier
        directory.mkdir(mode=0o700)
        rgb.save(directory / "source.png")
        canonicalize_capture(directory / "source.png", directory / "prepared.png", directory / "prepared.json")
        crop_left_half(directory / "prepared.png", directory / "crop-preview.png", directory / "crop-preview.json")
        data = {"id": identifier, "name": Path(name).name[:120], "source": source, "status": "draft",
                "created_at": now(), "started_at": None, "finished_at": None, "activity": "pipeline",
                "config": None, "stages": [], "error": None, "seconds": 0, "attacks": {}}
        job = Experiment(directory, data)
        job.update()
        with self.lock:
            self.jobs[identifier] = job
        return job

    def start(self, job, location):
        with job.lock:
            if job.data["status"] != "draft":
                raise HTTPException(409, "This capture is already frozen. Create a new experiment to change it.")
            env = runtime_env()
            region = describe_region(location)
            if not region["supported"]:
                raise HTTPException(422, region["reason"])
            if not region["contains_point"]:
                raise HTTPException(422, "The selected region does not contain the simulated capture point.")
            out = job.directory / "out"
            out.mkdir()
            region_path = job.directory / "region.json"
            write_json(region_path, region["region"])
            stages = build_stages(job.directory / "source.png", location.latitude, location.longitude, region_path, out, env)
            job.update(status="queued", config={**location.model_dump(), "region": region}, activity="pipeline",
                       stages=[{"id": s.label, "number": s.number, "title": s.title,
                                "description": s.explanation, "status": "pending", "seconds": None} for s in stages])
            job.emit("state", status="queued", message="Your experiment is queued. One prover runs at a time.")
            self.worker.submit(self.run, job, stages, env)

    def run(self, job, stages, env):
        started = time.monotonic()
        try:
            if job.cancel.is_set():
                raise Cancelled()
            job.update(status="running", started_at=now(), error=None)
            job.emit("state", status="running")
            for index, stage in enumerate(stages):
                job.data["stages"][index].update(status="running", started_at=now())
                job.update()
                job.emit("stage", stage=stage.label, status="running", title=stage.title)
                elapsed = job.execute(stage.label, stage.command, env)
                job.data["stages"][index].update(status="complete", seconds=elapsed)
                job.update()
                job.emit("stage", stage=stage.label, status="complete", seconds=elapsed)
            job.update(status="complete", finished_at=now(), seconds=time.monotonic() - started)
            job.emit("state", status="complete", message="Both proofs and all reader checks passed.")
        except Cancelled:
            self.fail(job, "cancelled", "Stopped by you. This run's files are preserved.")
        except Exception as error:
            self.fail(job, "failed", str(error))

    def fail(self, job, state, message):
        for stage in job.data["stages"]:
            if stage["status"] == "running":
                stage["status"] = state
        job.update(status=state, error=message, finished_at=now())
        job.emit("state", status=state, message=message)

    def action(self, job, action):
        with job.lock:
            if job.data["status"] != "complete":
                raise HTTPException(409, "Complete the pipeline before running this check.")
            job.cancel.clear()
            job.update(status="queued", activity=action, error=None)
            job.emit("state", status="queued", message="Queued: " + action)
            self.worker.submit(self.run_action, job, action)

    def run_action(self, job, action):
        try:
            env = runtime_env()
            out = job.directory / "out"
            job.update(status="running", started_at=now())
            job.emit("state", status="running")
            if action == "verify":
                stage = build_stages(job.directory / "source.png", 0, 0, job.directory / "region.json", out, env)[5]
                elapsed = job.execute("reader-verify", stage.command, env)
                job.update(reader_check={"time": now(), "seconds": elapsed, "passed": True})
                job.emit("message", stage="verify", message="Independent reader verification passed again.")
            else:
                job.execute("attack-" + action, [PIPELINE / ".venv/bin/python", WEB / "attack.py",
                            "--run", job.directory, "--case", action], env)
                report = read_json(out / "attacks" / action / "result.json")
                job.data["attacks"][action] = report
                job.emit("attack", result=report)
            job.update(status="complete", finished_at=now())
            job.emit("state", status="complete")
        except Cancelled:
            job.update(status="complete", error="The extra check was cancelled. The original proof bundle is unchanged.")
            job.emit("state", status="complete")
        except Exception as error:
            job.update(status="complete", error="The extra check failed: " + str(error))
            job.emit("state", status="complete", message=job.data["error"])

    def close(self):
        for job in self.jobs.values():
            if job.data["status"] not in TERMINAL:
                job.cancel.set()
                job.stop_process()
        self.worker.shutdown(wait=True, cancel_futures=True)


lab = Lab(RUNS)


@asynccontextmanager
async def lifespan(app):
    yield
    lab.close()


app = FastAPI(title="Provenance Lab", lifespan=lifespan)
app.add_middleware(TrustedHostMiddleware, allowed_hosts=["localhost", "127.0.0.1", "testserver"])


@app.middleware("http")
async def local_origin(request: Request, call_next):
    origin = request.headers.get("origin")
    if origin and urlparse(origin).hostname not in {"localhost", "127.0.0.1", "testserver"}:
        return JSONResponse({"detail": "This lab accepts requests from its local page only."}, status_code=403)
    response = await call_next(request)
    response.headers["X-Content-Type-Options"] = "nosniff"
    if request.url.path.startswith("/api/"):
        response.headers["Cache-Control"] = "no-store"
    return response


@app.get("/api/health")
def health():
    try:
        runtime_env()
        return {"ready": True, "mode": "local", "started": SERVER_STARTED}
    except (OSError, ValueError) as error:
        return {"ready": False, "error": str(error), "started": SERVER_STARTED}


@app.post("/api/regions")
def region(location: Location):
    return describe_region(location)


@app.get("/api/runs")
def experiments():
    with lab.lock:
        return sorted([job.snapshot() for job in lab.jobs.values()], key=lambda item: item["created_at"], reverse=True)[:30]


@app.post("/api/runs/sample")
def sample():
    path = ROOT / "fixtures/generated/demo-input.jpg"
    if not path.is_file():
        raise HTTPException(503, "Sample photo missing. Run ./pipeline/setup.sh first.")
    return lab.create(path.read_bytes(), "C2PA SDK sample.jpg").snapshot()


@app.post("/api/runs")
async def upload(file: UploadFile = File(...)):
    content = await file.read(MAX_UPLOAD + 1)
    await file.close()
    return await asyncio.to_thread(lambda: lab.create(content, file.filename or "Uploaded image").snapshot())


@app.get("/api/runs/{identifier}")
def experiment(identifier: str):
    return lab.get(identifier).snapshot()


@app.post("/api/runs/{identifier}/start")
def start(identifier: str, location: Location):
    job = lab.get(identifier)
    try:
        lab.start(job, location)
    except (OSError, ValueError) as error:
        raise HTTPException(503, str(error))
    return job.snapshot()


@app.post("/api/runs/{identifier}/cancel")
def cancel(identifier: str):
    job = lab.get(identifier)
    if job.data["status"] not in TERMINAL:
        job.cancel.set()
        job.stop_process()
        if job.data["status"] == "queued":
            job.update(status="cancelled", error="Stopped before this operation started.")
            job.emit("state", status="cancelled")
    return {"requested": True}


@app.post("/api/runs/{identifier}/actions/{action}")
def action(identifier: str, action: str):
    if action not in {"verify", "pixel", "signature", "region", "receipt"}:
        raise HTTPException(404, "Unknown check.")
    job = lab.get(identifier)
    lab.action(job, action)
    return {"status": job.data["status"], "activity": job.data["activity"]}


@app.get("/api/runs/{identifier}/events")
async def events(identifier: str, request: Request, after: int = 0):
    job = lab.get(identifier)
    try:
        after = max(after, int(request.headers.get("last-event-id", "0")))
    except ValueError:
        pass

    async def stream():
        cursor = max(0, after)
        heartbeat = time.monotonic()
        while not await request.is_disconnected():
            with job.lock:
                pending = job.events[cursor:]
            for event in pending:
                cursor = event["id"]
                yield "id: {}\ndata: {}\n\n".format(cursor, json.dumps(event))
            if time.monotonic() - heartbeat >= 10:
                yield ": keep-alive\n\n"
                heartbeat = time.monotonic()
            await asyncio.sleep(0.2)

    return StreamingResponse(stream(), media_type="text/event-stream", headers={"X-Accel-Buffering": "no"})


ASSETS = {
    "source.png": "source.png", "prepared.png": "prepared.png", "crop.png": "crop-preview.png",
    "signed.png": "out/signed.png", "manifest.json": "out/public-manifest.json",
    "receipt.json": "out/capture/receipt.json", "region.json": "region.json",
    "proof.bin": "out/pi-img/proof.bin", "location-proof.json": "out/pi-loc.json",
    "negative-results.json": "out/negative/results.json",
}


@app.get("/api/runs/{identifier}/assets/{name}")
def asset(identifier: str, name: str, download: bool = False):
    job = lab.get(identifier)
    if name == "manifest.json":
        public_manifest(job)
    if name not in ASSETS:
        raise HTTPException(404, "Artifact not found.")
    path = job.directory / ASSETS[name]
    if not path.is_file():
        raise HTTPException(404, "This artifact is not ready yet.")
    return FileResponse(path, filename=name if download else None)


@app.get("/api/runs/{identifier}/logs/{stage}")
def log(identifier: str, stage: str):
    job = lab.get(identifier)
    if not re.fullmatch(r"[a-z-]{1,60}", stage):
        raise HTTPException(404, "Log not found.")
    path = job.directory / "logs" / (stage + ".log")
    if not path.is_file():
        raise HTTPException(404, "Log not ready.")
    return FileResponse(path, media_type="text/plain", filename=stage + ".log")


@app.get("/api/runs/{identifier}/reader")
def reader(identifier: str):
    job = lab.get(identifier)
    out = job.directory / "out"
    manifest = public_manifest(job)
    assertion = assertion_from_manifest(manifest) if manifest else None
    pi_img = read_json(out / "pi-img/metadata.json")
    pi_loc = read_json(out / "pi-loc.json")
    signed = out / "signed.png"
    return {
        "id": identifier, "published": signed.is_file(), "assertion": assertion,
        "manifest": manifest, "image_proof": pi_img,
        "sizes": {
            "image_proof": sum(f["bytes"] for f in pi_img["files"].values()) if pi_img else None,
            "location_proof": len(base64.b64decode(pi_loc["proof"])) if pi_loc else None,
            "signed_png": signed.stat().st_size if signed.is_file() else None,
        },
        "receipt_benchmark": read_json(out / "receipt-verification.json"),
        "negative_results": read_json(out / "negative/results.json"),
        "attacks": job.data["attacks"], "reader_check": job.data.get("reader_check"),
        "stages": job.snapshot()["stages"], "status": job.data["status"],
        "activity": job.data["activity"], "started_at": job.data["started_at"],
        "error": "This operation did not finish. Return to capture view for the local diagnostic." if job.data["error"] else None,
    }


def public_manifest(job):
    path = job.directory / "out/public-manifest.json"
    signed = job.directory / "out/signed.png"
    if not signed.is_file():
        return None
    with job.lock:
        cached = read_json(path)
        if cached is not None:
            return cached
        binary = os.environ.get("C2PATOOL", str(Path(os.environ.get("C2PA_BIN_DIR", str(Path.home() / ".local/bin"))) / "c2patool"))
        completed = subprocess.run([binary, str(signed)], capture_output=True, text=True, timeout=20)
        if completed.returncode:
            return None
        try:
            manifest = json.loads(completed.stdout)
        except json.JSONDecodeError:
            return None
        write_json(path, manifest)
        return manifest


@app.get("/api/runs/{identifier}/matrix")
def matrix(identifier: str, channel: str = "R", x: int = 0, y: int = 0):
    job = lab.get(identifier)
    if channel not in "RGB" or len(channel) != 1 or not 0 <= x <= 1016 or not 0 <= y <= 504:
        raise HTTPException(422, "Choose R, G or B and an 8 by 8 patch inside the image.")
    with Image.open(job.directory / "prepared.png") as image:
        pixels = list(image.crop((x, y, x + 8, y + 8)).getdata())
    fingerprint = read_json(job.directory / "out/capture/fingerprint.json")
    return {"channel": channel, "x": x, "y": y, "pixels": pixels,
            "values": [pixel["RGB".index(channel)] for pixel in pixels],
            "indices": [(y + row) * 1024 + x + col for row in range(8) for col in range(8)],
            "fingerprint": fingerprint["channels"][channel] if fingerprint else None,
            "field": "BLS12-381 scalar", "input_length": 524288, "output_length": 128}


SERVER_STARTED = now()
if (WEB / "dist").is_dir():
    app.mount("/", StaticFiles(directory=WEB / "dist", html=True), name="web")
