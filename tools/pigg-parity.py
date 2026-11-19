"""Check crates/coh_pigg against the Python Pigg Wrangler, byte for byte.

Hashes every entry of every .pigg in a folder with both readers and reports any difference.
Needs real game files, so it runs by hand, not in CI.

    python3 tools/pigg-parity.py ~/.wine/drive_c/Games/Homecoming/assets/live
"""

from __future__ import annotations

import hashlib
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(REPO / "tools" / "pigg-wrangler"))

from pigg_wrangler.pigg import PiggArchive  # noqa: E402


def python_lines(folder: Path) -> list[str]:
    lines = []
    for bundle in sorted(folder.glob("*.pigg")):
        archive = PiggArchive(bundle)
        for index, entry in enumerate(archive.entries):
            data = archive.extract(entry)
            digest = hashlib.sha256(data).hexdigest()
            lines.append(f"{bundle.name}\t{index}\t{entry.path}\t{len(data)}\t{digest}")
    return lines


def rust_lines(folder: Path) -> list[str]:
    command = [
        "cargo",
        "run",
        "-q",
        "--release",
        "-p",
        "coh_pigg",
        "--example",
        "pigg_hashes",
        "--",
        str(folder),
    ]
    result = subprocess.run(
        command, cwd=REPO, capture_output=True, text=True, check=False
    )
    if result.returncode != 0:
        sys.exit(f"the Rust reader failed:\n{result.stderr}")
    return result.stdout.splitlines()


def main() -> None:
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    folder = Path(sys.argv[1]).expanduser()
    python, rust = python_lines(folder), rust_lines(folder)
    bundles = len({line.split("\t", 1)[0] for line in python})
    mismatches = [(p, r) for p, r in zip(python, rust, strict=False) if p != r]
    print(
        f"{bundles} bundles, {len(python)} entries (Python), {len(rust)} entries (Rust), {len(mismatches)} mismatched"
    )
    for p, r in mismatches[:10]:
        print(f"  python: {p}\n  rust:   {r}")
    if mismatches or len(python) != len(rust) or not python:
        sys.exit(1)


if __name__ == "__main__":
    main()
