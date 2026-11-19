"""Check crates/coh_pigg against the Python Pigg Wrangler.

Files: hashes every entry of every .pigg in a folder with both readers.
Textures (--textures): decodes every .texture in those bundles with both, compares pixels, then
does the same for generated images in every format texture.py knows, including the ones no
shipped game uses.
Zips (--zips): zips every bundle whole in Rust, as the page zips a folder, then opens each zip with
Python's zipfile and `unzip -t` and checks every file in it against the Python reader.
Needs real game files, so it runs by hand, not in CI.

    python3 tools/pigg-parity.py ~/.wine/drive_c/Games/Homecoming/assets/live
    python3 tools/pigg-parity.py --textures ~/.wine/drive_c/Games/Homecoming/assets/live
    python3 tools/pigg-parity.py --zips ~/.wine/drive_c/Games/Homecoming/assets/live
"""

from __future__ import annotations

import collections
import hashlib
import random
import struct
import subprocess
import sys
import tempfile
from multiprocessing import Pool
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO / "tools" / "pigg-wrangler"))

from pigg_wrangler import texture  # noqa: E402
from pigg_wrangler.pigg import PiggArchive  # noqa: E402


def sha(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def rust(example: str, folder: Path) -> list[str]:
    command = ["cargo", "run", "-q", "--release", "-p", "coh_pigg"]
    command += ["--example", example, "--", str(folder)]
    result = subprocess.run(
        command, cwd=REPO, capture_output=True, text=True, check=False
    )
    if result.returncode != 0:
        sys.exit(f"the Rust {example} failed:\n{result.stderr}")
    return result.stdout.splitlines()


def compare(label: str, python: list[str], rust_out: list[str]) -> bool:
    mismatches = [(p, r) for p, r in zip(python, rust_out, strict=False) if p != r]
    print(
        f"{label}: {len(python)} (Python), {len(rust_out)} (Rust), "
        f"{len(mismatches)} mismatched"
    )
    for p, r in mismatches[:10]:
        print(f"  python: {p}\n  rust:   {r}")
    return not mismatches and len(python) == len(rust_out) and bool(python)


# ---- files ----


def file_lines(folder: Path) -> list[str]:
    lines = []
    for bundle in sorted(folder.glob("*.pigg")):
        archive = PiggArchive(bundle)
        for index, entry in enumerate(archive.entries):
            data = archive.extract(entry)
            lines.append(
                f"{bundle.name}\t{index}\t{entry.path}\t{len(data)}\t{sha(data)}"
            )
    return lines


# ---- textures ----


def describe(data: bytes) -> str:
    try:
        info = texture.parse_texture(data)
        if info.image_format == "jpeg":
            return f"jpeg\t{sha(texture.texture_to_jpeg(data))}"
        rgba, width, height, dds = texture.decode_dds_to_rgba(
            texture.texture_to_dds(data)
        )
    except (ValueError, struct.error, IndexError):
        return "refused"
    return f"{dds.pixel_format}\t{width}x{height}\t{sha(rgba)}"


_archives: dict[Path, PiggArchive] = {}


def describe_entry(task: tuple[Path, int]) -> str:
    bundle, index = task
    archive = _archives.setdefault(bundle, PiggArchive(bundle))
    entry = archive.entries[index]
    return f"{bundle.name}\t{entry.path}\t{describe(archive.extract(entry))}"


def texture_lines(folder: Path) -> list[str]:
    tasks = []
    for bundle in sorted(folder.glob("*.pigg")):
        for index, entry in enumerate(PiggArchive(bundle).entries):
            if entry.path.lower().endswith(".texture"):
                tasks.append((bundle, index))
    with Pool() as pool:
        lines = pool.map(describe_entry, tasks, chunksize=8)
    for loose in sorted(folder.glob("*.texture")):
        lines.append(f"loose\t{loose.name}\t{describe(loose.read_bytes())}")
    return lines


FOURCC, RGB, ALPHA_PIXELS, LUMINANCE = 0x4, 0x40, 0x1, 0x20000
# name: (pixel-format flags, FourCC, bit count, r/g/b/a masks, bytes per pixel or per block)
FORMATS = {
    "DXT1": (FOURCC, b"DXT1", 0, (0, 0, 0, 0), ("block", 8)),
    "NVT3": (FOURCC, b"NVT3", 0, (0, 0, 0, 0), ("block", 8)),
    "DXT3": (FOURCC, b"DXT3", 0, (0, 0, 0, 0), ("block", 16)),
    "DXT5": (FOURCC, b"DXT5", 0, (0, 0, 0, 0), ("block", 16)),
    "BGRA": (
        RGB | ALPHA_PIXELS,
        b"\0" * 4,
        32,
        (0xFF0000, 0xFF00, 0xFF, 0xFF000000),
        ("pixel", 4),
    ),
    "BGRX": (RGB, b"\0" * 4, 32, (0xFF0000, 0xFF00, 0xFF, 0), ("pixel", 4)),
    "RGBA": (
        RGB | ALPHA_PIXELS,
        b"\0" * 4,
        32,
        (0xFF, 0xFF00, 0xFF0000, 0xFF000000),
        ("pixel", 4),
    ),
    "RGBX": (RGB, b"\0" * 4, 32, (0xFF, 0xFF00, 0xFF0000, 0), ("pixel", 4)),
    "BGR": (RGB, b"\0" * 4, 24, (0xFF0000, 0xFF00, 0xFF, 0), ("pixel", 3)),
    "RGB": (RGB, b"\0" * 4, 24, (0xFF, 0xFF00, 0xFF0000, 0), ("pixel", 3)),
    "L8": (LUMINANCE, b"\0" * 4, 8, (0xFF, 0, 0, 0), ("pixel", 1)),
    # Both readers must refuse these.
    "RGB565": (RGB, b"\0" * 4, 16, (0xF800, 0x7E0, 0x1F, 0), ("pixel", 2)),
    "L16": (LUMINANCE, b"\0" * 4, 16, (0xFFFF, 0, 0, 0), ("pixel", 2)),
    "DX10": (FOURCC, b"DX10", 0, (0, 0, 0, 0), ("block", 16)),
}
SIZES = [(1, 1), (4, 4), (5, 3), (13, 7), (16, 8)]


def wrap_texture(payload: bytes) -> bytes:
    header = bytearray(64)
    struct.pack_into("<I", header, 0, len(header))
    header[0x1D:0x20] = b"TX2"
    header[0x20:0x2A] = b"gen/x.dds\0"
    return bytes(header) + payload


def make_dds(name: str, width: int, height: int, rng: random.Random) -> bytes:
    flags, fourcc, bits, masks, (unit, size) = FORMATS[name]
    dds = bytearray(128)
    dds[0:4] = b"DDS "
    struct.pack_into("<III", dds, 8, 0, height, width)
    struct.pack_into("<I4sI4I", dds, 80, flags, fourcc, bits, *masks)
    if unit == "block":
        count = ((width + 3) // 4) * ((height + 3) // 4)
    else:
        count = width * height
    return bytes(dds) + rng.randbytes(count * size)


def write_generated(folder: Path) -> int:
    rng = random.Random(20261119)
    count = 0
    for name in FORMATS:
        for width, height in SIZES:
            for copy in range(4):
                path = folder / f"{name}_{width}x{height}_{copy}.texture"
                path.write_bytes(wrap_texture(make_dds(name, width, height, rng)))
                count += 1
    (folder / "jpeg_0.texture").write_bytes(
        wrap_texture(b"\xff\xd8" + rng.randbytes(200))
    )
    return count + 1


def check_textures(folder: Path) -> bool:
    python = texture_lines(folder)
    formats = collections.Counter(line.split("\t")[2] for line in python)
    print("real textures by format:", dict(formats))
    real_ok = compare("real textures", python, rust("texture_hashes", folder))
    with tempfile.TemporaryDirectory() as scratch:
        generated = Path(scratch)
        write_generated(generated)
        python = texture_lines(generated)
        formats = collections.Counter(line.split("\t")[2] for line in python)
        print("generated textures by result:", dict(formats))
        generated_ok = compare(
            "generated textures", python, rust("texture_hashes", generated)
        )
    return real_ok and generated_ok


# ---- zips ----


def zip_lines(folder: Path) -> tuple[list[str], list[str]]:
    """(Python's extraction, what the Rust zips hold), one line per file, in bundle order."""
    import zipfile

    python, rust_out = [], []
    with tempfile.TemporaryDirectory() as out:
        rust_zips = Path(out)
        command = ["cargo", "run", "-q", "--release", "-p", "coh_pigg"]
        command += ["--example", "zip_bundles", "--", str(folder), str(rust_zips)]
        subprocess.run(command, cwd=REPO, check=True)
        for bundle in sorted(folder.glob("*.pigg")):
            archive = PiggArchive(bundle)
            for entry in archive.entries:
                data = archive.extract(entry)
                python.append(f"{bundle.name}\t{entry.path}\t{len(data)}\t{sha(data)}")
            made = rust_zips / f"{bundle.stem}.zip"
            tested = subprocess.run(["unzip", "-tqq", str(made)], check=False)
            with zipfile.ZipFile(made) as zf:
                bad = zf.testzip()
                for info in zf.infolist():
                    data = zf.read(info)
                    rust_out.append(
                        f"{bundle.name}\t{info.filename}\t{len(data)}\t{sha(data)}"
                    )
            if tested.returncode != 0 or bad is not None:
                rust_out.append(
                    f"{bundle.name}\tunzip -t exit {tested.returncode}, testzip {bad}"
                )
    return python, rust_out


def main() -> None:
    args = sys.argv[1:]
    textures = "--textures" in args
    zips = "--zips" in args
    args = [arg for arg in args if arg not in ("--textures", "--zips")]
    if len(args) != 1:
        sys.exit(__doc__)
    folder = Path(args[0]).expanduser()
    if textures:
        ok = check_textures(folder)
    elif zips:
        ok = compare("zipped files", *zip_lines(folder))
    else:
        ok = compare("files", file_lines(folder), rust("pigg_hashes", folder))
    if not ok:
        sys.exit(1)


if __name__ == "__main__":
    main()
