"""Generate pre-linearization tile references with an independent SOF3 decoder.

Requires numpy==2.2.6, tifffile==2025.5.10, imagecodecs==2025.3.30.
Usage: python3 tests/proraw_reference.py INPUT.dng NEW_OUTPUT_DIRECTORY
The manifest records source and tile hashes; no photographic rendering is applied.
"""

import hashlib
import json
from pathlib import Path
import sys

import imagecodecs
import numpy
import tifffile


def main():
    source, destination = map(Path, sys.argv[1:])
    versions = (numpy.__version__, tifffile.__version__, imagecodecs.__version__)
    if versions != ("2.2.6", "2025.5.10", "2025.3.30"):
        raise RuntimeError(f"Unexpected reference dependency versions: {versions}")
    data = source.read_bytes()
    with tifffile.TiffFile(source) as container:
        pages = list(container.pages)
        for page in list(pages):
            if page.pages:
                pages.extend(page.pages)
        candidates = [page for page in pages if page.subfiletype == 0 and page.photometric == 34892]
        if len(candidates) != 1:
            raise ValueError("Expected exactly one primary LinearRaw image")
        page = candidates[0]
        if page.compression != 7 or page.samplesperpixel != 3:
            raise ValueError("Expected lossless-JPEG three-channel storage")
        destination.mkdir()
        tiles = []
        manifest = []
        for index, (offset, size) in enumerate(zip(page.dataoffsets, page.databytecounts, strict=True)):
            decoded = imagecodecs.jpegsof3_decode(bytearray(data[offset:offset + size]))
            if decoded.shape != (page.tilelength, page.tilewidth, 3) or decoded.dtype != numpy.uint16:
                raise ValueError(f"Unexpected tile layout: {decoded.shape}, {decoded.dtype}")
            samples = decoded.astype("<u2").tobytes(order="C")
            name = f"tile-{index}.u16le"
            (destination / name).write_bytes(samples)
            manifest.append(f"{offset} {size} {page.tilewidth} {page.tilelength} {name}\n")
            tiles.append({"file": name, "sha256": hashlib.sha256(samples).hexdigest()})
        (destination / "tiles.txt").write_text("".join(manifest))
        provenance = {
            "source": str(source.resolve()),
            "sha256": hashlib.sha256(data).hexdigest(),
            "versions": versions,
            "decoder": imagecodecs.jpegsof3_version(),
            "tiles": tiles,
        }
        (destination / "provenance.json").write_text(json.dumps(provenance, indent=2) + "\n")
        print(f"{source.name}: {len(tiles)} independently decoded tiles")


if __name__ == "__main__":
    main()
