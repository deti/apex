"""Thin wrapper that downloads and runs the apex binary."""

import os
import platform
import subprocess
import sys
import tarfile
import tempfile
import urllib.request
from pathlib import Path

__version__ = "0.3.1"

REPO = "sahajamoth/apex"
BINARY = "apex"

# Binary is cached next to this package
_BIN_DIR = Path(__file__).parent / "bin"
_BIN_PATH = _BIN_DIR / BINARY


def _get_target() -> str:
    system = platform.system()
    machine = platform.machine()

    os_map = {"Darwin": "apple-darwin", "Linux": "unknown-linux-gnu"}
    arch_map = {"x86_64": "x86_64", "AMD64": "x86_64", "arm64": "aarch64", "aarch64": "aarch64"}

    target_os = os_map.get(system)
    target_arch = arch_map.get(machine)

    if not target_os or not target_arch:
        raise RuntimeError(f"Unsupported platform: {system}-{machine}")

    return f"{target_arch}-{target_os}"


def _ensure_binary() -> Path:
    if _BIN_PATH.exists():
        return _BIN_PATH

    target = _get_target()
    url = f"https://github.com/{REPO}/releases/download/v{__version__}/{BINARY}-{target}.tar.gz"

    print(f"Downloading {BINARY} v{__version__} ({target})...", file=sys.stderr)

    _BIN_DIR.mkdir(parents=True, exist_ok=True)

    with tempfile.TemporaryDirectory() as tmpdir:
        archive = os.path.join(tmpdir, "archive.tar.gz")
        urllib.request.urlretrieve(url, archive)

        with tarfile.open(archive, "r:gz") as tf:
            tf.extract(BINARY, tmpdir)

        src = os.path.join(tmpdir, BINARY)
        os.chmod(src, 0o755)
        # Atomic move
        tmp_dest = _BIN_PATH.with_suffix(".tmp")
        os.replace(src, tmp_dest)
        os.replace(tmp_dest, _BIN_PATH)

    return _BIN_PATH


def main() -> None:
    binary = _ensure_binary()
    result = subprocess.run([str(binary)] + sys.argv[1:])
    raise SystemExit(result.returncode)
