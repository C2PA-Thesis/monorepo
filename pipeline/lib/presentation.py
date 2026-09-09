from __future__ import annotations

import json
import os
import queue
import re
import shlex
import shutil
import signal
import subprocess
import sys
import textwrap
import threading
import time
from collections import deque
from pathlib import Path


EVENT_PREFIX = "::poc:: "


def announce(message: str) -> None:
    if os.environ.get("PIPELINE_EXPLAIN") == "1":
        print(EVENT_PREFIX + message, flush=True)


def clean(text: str) -> str:
    text = re.sub(r"\x1b\[[0-?]*[ -/]*[@-~]", "", text)
    return "".join(char for char in text if char.isprintable() or char in "\n\t")


def duration(seconds: float) -> str:
    minutes, seconds = divmod(int(seconds), 60)
    return "{}:{:02d}".format(minutes, seconds)


class StepFailed(Exception):
    def __init__(self, name: str, returncode: int, log: Path):
        self.returncode = returncode if returncode > 0 else 128 - returncode
        super().__init__("{} stopped (exit {}). Log: {}".format(name, self.returncode, log))


class Console:
    def __init__(self, plain: bool = False, verbose: bool = False):
        self.terminal = sys.stdout.isatty() and os.environ.get("TERM") != "dumb"
        self.animate = self.terminal and not plain
        self.color = self.animate and "NO_COLOR" not in os.environ
        self.verbose = verbose
        columns = shutil.get_terminal_size((88, 24)).columns or 88
        self.width = max(24, min(columns, 100))
        self.active = False

    def style(self, text: str, code: str) -> str:
        return "\033[{}m{}\033[0m".format(code, text) if self.color else text

    def clear(self) -> None:
        if self.active:
            print("\r\033[2K", end="", flush=True)
            self.active = False

    def line(self, text: str = "", code: str = "0") -> None:
        self.clear()
        print(self.style(clean(text), code), flush=True)

    def detail(self, text: str) -> None:
        for line in textwrap.wrap(clean(text), max(20, self.width - 4)):
            self.line("    " + line, "2")

    def field(self, label: str, value: object) -> None:
        self.line("  " + label, "1")
        if isinstance(value, Path):
            self.line("    " + str(value), "2")
        else:
            self.detail(str(value))

    def stage(self, number: int, title: str, explanation: str) -> None:
        bar = "[" + "=" * (number - 1) + ">" + "." * (8 - number) + "]"
        self.line()
        self.line("  {}  {}/8".format(bar, number), "1;36")
        for line in textwrap.wrap(title, self.width - 2):
            self.line("  " + line, "1;36")
        self.detail(explanation)

    def run(self, name: str, command: list, log: Path, env: dict, cwd: Path) -> float:
        command = [str(item) for item in command]
        if self.verbose:
            self.field("Command", shlex.join(command))
        self.field("Full log", log)
        log.parent.mkdir(parents=True, exist_ok=True)
        started = time.monotonic()
        lines = queue.Queue()
        tail = deque(maxlen=12)
        with log.open("w", encoding="utf-8") as output:
            output.write("Command: " + shlex.join(command) + "\n\n")
            output.flush()
            process = subprocess.Popen(
                command, cwd=cwd, env=env, stdout=subprocess.PIPE,
                stderr=subprocess.STDOUT, text=True, encoding="utf-8",
                errors="replace", start_new_session=True,
            )

            def read_output() -> None:
                try:
                    for line in process.stdout:
                        output.write(line)
                        output.flush()
                        lines.put(line.rstrip("\n"))
                finally:
                    lines.put(None)

            reader = threading.Thread(target=read_output, daemon=True)
            reader.start()
            last_heartbeat = started
            output_finished = False
            try:
                while True:
                    try:
                        line = lines.get(timeout=0.1)
                    except queue.Empty:
                        line = ""
                    if line is None:
                        output_finished = True
                    if output_finished and process.poll() is not None:
                        break
                    if line:
                        tail.append(clean(line))
                        if line.startswith(EVENT_PREFIX):
                            self.detail("-> " + line[len(EVENT_PREFIX):])
                        elif line.startswith("==> "):
                            self.detail("-> " + line[4:])
                        elif self.verbose:
                            self.detail(line)
                    now = time.monotonic()
                    elapsed = now - started
                    if self.animate:
                        frame = "|/-\\"[int(elapsed * 8) % 4]
                        status = "  {} {} elapsed | {}".format(frame, duration(elapsed), name)
                        print("\r\033[2K" + self.style(status[:self.width - 1], "36"), end="", flush=True)
                        self.active = True
                    elif now - last_heartbeat >= 15:
                        self.detail("Still running: {}. Elapsed {}.".format(name, duration(elapsed)))
                        last_heartbeat = now
                returncode = process.wait()
            except BaseException:
                try:
                    os.killpg(process.pid, signal.SIGTERM)
                except ProcessLookupError:
                    pass
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    pass
                reader.join(timeout=1)
                if process.poll() is None or reader.is_alive():
                    try:
                        os.killpg(process.pid, signal.SIGKILL)
                    except ProcessLookupError:
                        pass
                process.wait()
                raise
            finally:
                reader.join(timeout=2)
                process.stdout.close()
                self.clear()
            elapsed = time.monotonic() - started
            if returncode:
                self.line("  [FAILED] " + name, "1;31")
                for line in tail:
                    self.detail(line)
                raise StepFailed(name, returncode, log)
        self.line("  [OK] {} ({:.2f}s)".format(name, elapsed), "1;32")
        return elapsed

    def preview(self, path: Path, python: Path, label: str) -> None:
        code = """
import json, sys
from PIL import Image
with Image.open(sys.argv[1]) as image:
    result = dict(width=image.width, height=image.height, format=image.format)
    thumbnail = image.convert('RGB')
    scale = min(int(sys.argv[2]) / image.width, 24 / image.height)
    width = max(1, round(image.width * scale))
    height = max(1, round(image.height * scale / 2))
    thumbnail = thumbnail.resize((width, height))
    result.update(pixels=list(thumbnail.getdata()), columns=width)
    print(json.dumps(result))
"""
        result = subprocess.run(
            [str(python), "-c", code, str(path), str(min(40, self.width - 6))],
            capture_output=True, text=True,
        )
        if result.returncode:
            reason = result.stderr.strip().splitlines()
            raise ValueError("Cannot read image {}: {}".format(
                path, reason[-1] if reason else "image inspection failed",
            ))
        data = json.loads(result.stdout)
        self.field(label, path)
        self.detail("{} | {} x {} pixels | {:,} bytes".format(data["format"], data["width"], data["height"], path.stat().st_size))
        if self.terminal:
            ramp = " .:-=+*#%@"
            pixels, columns = data["pixels"], data["columns"]
            for start in range(0, len(pixels), columns):
                row = "    "
                for red, green, blue in pixels[start:start + columns]:
                    if self.color:
                        index = 16 + 36 * round(red / 51) + 6 * round(green / 51) + round(blue / 51)
                        row += "\033[48;5;{}m ".format(index)
                    else:
                        row += ramp[round((red * .299 + green * .587 + blue * .114) / 255 * (len(ramp) - 1))]
                print(row + ("\033[0m" if self.color else ""), flush=True)
