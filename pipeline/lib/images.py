from __future__ import annotations

from pathlib import Path
from typing import Any, Dict, Tuple

from PIL import Image

from .contracts import write_json


ORIGINAL_SIZE = (1024, 512)
CROP_SIZE = (512, 512)


def _resampling_filter() -> int:
    if hasattr(Image, "Resampling"):
        return Image.Resampling.LANCZOS
    return Image.LANCZOS


def save_lossless_rgb(image: Image.Image, path: Path) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    image.convert("RGB").save(
        path,
        format="PNG",
        compress_level=9,
        optimize=False,
    )


def canonicalize_capture(source: Path, png_path: Path, json_path: Path) -> None:
    with Image.open(source) as image:
        rgb = image.convert("RGB").resize(ORIGINAL_SIZE, _resampling_filter())
        save_lossless_rgb(rgb, png_path)
    png_to_hv_json(png_path, json_path, ORIGINAL_SIZE)


def crop_left_half(source_png: Path, png_path: Path, json_path: Path) -> None:
    with Image.open(source_png) as image:
        rgb = image.convert("RGB")
        if rgb.size != ORIGINAL_SIZE:
            raise ValueError(
                "capture must be {}x{}, got {}x{}".format(
                    ORIGINAL_SIZE[0],
                    ORIGINAL_SIZE[1],
                    rgb.width,
                    rgb.height,
                )
            )
        cropped = rgb.crop((0, 0, CROP_SIZE[0], CROP_SIZE[1]))
        save_lossless_rgb(cropped, png_path)
    png_to_hv_json(png_path, json_path, CROP_SIZE)


def png_to_hv_json(
    png_path: Path,
    json_path: Path,
    expected_size: Tuple[int, int],
) -> Dict[str, Any]:
    with Image.open(png_path) as image:
        if image.format != "PNG":
            raise ValueError("published asset must be a PNG")
        rgb = image.convert("RGB")
        if rgb.size != expected_size:
            raise ValueError(
                "expected {}x{} PNG, got {}x{}".format(
                    expected_size[0],
                    expected_size[1],
                    rgb.width,
                    rgb.height,
                )
            )
        pixels = list(rgb.getdata())

    document = {
        "rows": expected_size[0],
        "cols": expected_size[1],
        "R": [pixel[0] for pixel in pixels],
        "G": [pixel[1] for pixel in pixels],
        "B": [pixel[2] for pixel in pixels],
    }
    write_json(json_path, document)
    return document
