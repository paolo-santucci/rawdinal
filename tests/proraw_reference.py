"""Generate independent SOF3 references and a separate DNG normalization formula reference.

Requires numpy==2.2.6, tifffile==2025.5.10, imagecodecs==2025.3.30.
Usage: python3 tests/proraw_reference.py INPUT.dng NEW_OUTPUT_DIRECTORY
The formula reference is not an Adobe SDK or Apple rendering oracle.
"""

import hashlib
import json
from pathlib import Path
import sys

import imagecodecs
import numpy
import tifffile


def pages_recursive(pages):
    for page in pages:
        yield page
        if page.pages:
            yield from pages_recursive(page.pages)


def numbers(page, code, default):
    tag = page.tags.get(code)
    if tag is None:
        return numpy.asarray(default, dtype=numpy.float64)
    values = numpy.asarray(tag.value, dtype=numpy.float64).reshape(-1)
    if tag.dtype in (5, 10) and len(values) == 2 * tag.count:
        values = values[::2] / values[1::2]
    if len(values) != tag.count or not numpy.isfinite(values).all():
        raise ValueError(f"Invalid numeric tag {code}")
    return values


def normalized_reference(page, encoded):
    height, width, channels = encoded.shape
    active = numbers(page, 50829, [0, 0, height, width]).astype(int)
    top, left, bottom, right = active
    repeat_rows, repeat_columns = numbers(page, 50713, [1, 1]).astype(int)
    black = numbers(page, 50714, numpy.zeros(repeat_rows * repeat_columns * channels))
    black = black.reshape(repeat_rows, repeat_columns, channels)
    horizontal = numbers(page, 50715, numpy.zeros(right - left))
    vertical = numbers(page, 50716, numpy.zeros(bottom - top))
    white = numbers(page, 50717, [2**page.bitspersample - 1] * channels)
    max_black = numpy.full(channels, -numpy.inf)
    for row in range(repeat_rows):
        for column in range(repeat_columns):
            candidate = black[row, column] + horizontal[column::repeat_columns].max() + vertical[row::repeat_rows].max()
            max_black = numpy.maximum(max_black, candidate)
    denominator = white - max_black
    if numpy.any(denominator <= 0):
        raise ValueError("Invalid normalization denominator")
    linearization = page.tags.get(50712)
    result = numpy.empty(encoded.shape, dtype="<f4")
    columns = numpy.clip(numpy.arange(width), left, right - 1) - left
    for row in range(height):
        active_row = min(max(row, top), bottom - 1) - top
        values = encoded[row].astype(numpy.float64)
        if linearization is not None:
            table = numpy.asarray(linearization.value)
            values = table[numpy.minimum(encoded[row], len(table) - 1)].astype(numpy.float64)
        bias = black[active_row % repeat_rows, columns % repeat_columns] + horizontal[columns, None] + vertical[active_row]
        result[row] = (values - bias) / denominator
    return result


def write_reference(destination, name, data):
    with (destination / name).open("xb") as stream:
        stream.write(data)
    return {"file": name, "sha256": hashlib.sha256(data).hexdigest()}


def main():
    source, destination = map(Path, sys.argv[1:])
    versions = (numpy.__version__, tifffile.__version__, imagecodecs.__version__)
    if versions != ("2.2.6", "2025.5.10", "2025.3.30"):
        raise RuntimeError(f"Unexpected reference dependency versions: {versions}")
    data = source.read_bytes()
    with tifffile.TiffFile(source) as container:
        pages = list(pages_recursive(container.pages))
        candidates = [page for page in pages if page.subfiletype == 0 and page.photometric == 34892]
        if len(candidates) != 1:
            raise ValueError("Expected exactly one primary LinearRaw image")
        page = candidates[0]
        if page.compression != 7 or page.samplesperpixel != 3:
            raise ValueError("Expected lossless-JPEG three-channel storage")
        if page.planarconfig != 1 or not page.is_tiled:
            raise ValueError("This reference generator requires chunky tiled storage")
        if any(code in page.tags for code in (51008, 51009, 51022)):
            raise ValueError("The formula reference does not implement opcodes")
        destination.mkdir()
        tiles = []
        manifest = []
        encoded = numpy.empty((page.imagelength, page.imagewidth, 3), dtype="<u2")
        columns = (page.imagewidth + page.tilewidth - 1) // page.tilewidth
        for index, (offset, size) in enumerate(zip(page.dataoffsets, page.databytecounts, strict=True)):
            decoded = imagecodecs.jpegsof3_decode(bytearray(data[offset:offset + size]))
            if decoded.shape != (page.tilelength, page.tilewidth, 3) or decoded.dtype != numpy.uint16:
                raise ValueError(f"Unexpected tile layout: {decoded.shape}, {decoded.dtype}")
            samples = decoded.astype("<u2").tobytes(order="C")
            name = f"tile-{index}.u16le"
            record = write_reference(destination, name, samples)
            record.update(offset=offset, size=size, width=page.tilewidth, height=page.tilelength)
            manifest.append(f"{offset} {size} {page.tilewidth} {page.tilelength} {name}\n")
            tiles.append(record)
            y, x = (index // columns) * page.tilelength, (index % columns) * page.tilewidth
            height, width = min(page.tilelength, page.imagelength - y), min(page.tilewidth, page.imagewidth - x)
            encoded[y:y + height, x:x + width] = decoded[:height, :width]
        manifest_bytes = "".join(manifest).encode()
        write_reference(destination, "tiles.txt", manifest_bytes)
        normalized = normalized_reference(page, encoded)
        stages = [write_reference(destination, "encoded.u16le", encoded.tobytes()), write_reference(destination, "normalized.f32le", normalized.tobytes())]
        root = container.pages[0]
        provenance = {
            "schema": 2,
            "source": str(source.resolve()),
            "sha256": hashlib.sha256(data).hexdigest(),
            "versions": versions,
            "decoder": imagecodecs.jpegsof3_version(),
            "tiles": tiles,
            "manifest_sha256": hashlib.sha256(manifest_bytes).hexdigest(),
            "stages": stages,
            "normalization": "float64 formula, per-channel global maximum black; unclamped float32 output; no opcodes",
            "width": page.imagewidth,
            "height": page.imagelength,
            "bits": page.bitspersample,
            "compression": int(page.compression),
            "raw_ifd_offset": page.offset,
            "camera": {str(code): str(root.tags[code].value) for code in (271, 272, 305, 306, 50706, 50707) if code in root.tags},
        }
        (destination / "provenance.json").write_text(json.dumps(provenance, indent=2) + "\n")
        print(f"{source.name}: {page.imagewidth}x{page.imagelength}, {len(tiles)} independent tiles and full-image formula reference")


if __name__ == "__main__":
    main()
