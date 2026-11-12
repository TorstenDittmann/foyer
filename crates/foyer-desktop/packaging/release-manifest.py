"""Writes foyer-desktop.json, the manifest the client checks for updates.

    python3 release-manifest.py --version 0.2.0 --dist dist \
        --base-url https://github.com/OWNER/REPO/releases/download/desktop-v0.2.0 \
        --notes-url https://github.com/OWNER/REPO/releases/tag/desktop-v0.2.0 \
        [--secret-key minisign.key]

With --secret-key every file is signed with `minisign`, once per platform,
with the trusted comment "foyer-desktop <version> <platform>" that the client
checks (see crates/foyer-install/src/client_update.rs). Also writes
SHA256SUMS for people checking downloads by hand.
"""

import argparse
import datetime
import hashlib
import json
import subprocess
import sys
import tempfile
from pathlib import Path

# Platform key (client_update::target()) -> file the client installs.
UPDATE_FILES = {
    "windows-x86_64": "Foyer-Setup-x64.exe",
    "linux-x86_64": "Foyer-x86_64.AppImage",
    "macos-aarch64": "Foyer-macos.app.tar.gz",
    "macos-x86_64": "Foyer-macos.app.tar.gz",
}


def sha256(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def sign(path: Path, key: Path, comment: str) -> str:
    with tempfile.TemporaryDirectory() as tmp:
        out = Path(tmp) / "file.minisig"
        subprocess.run(
            ["minisign", "-S", "-s", str(key), "-m", str(path), "-x", str(out), "-t", comment],
            check=True,
            stdin=subprocess.DEVNULL,
        )
        return out.read_text()


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--version", required=True)
    parser.add_argument("--dist", type=Path, required=True)
    parser.add_argument("--base-url", required=True)
    parser.add_argument("--notes-url")
    parser.add_argument("--secret-key", type=Path)
    args = parser.parse_args()

    assets = {}
    for target, name in UPDATE_FILES.items():
        path = args.dist / name
        if not path.is_file():
            sys.exit(f"missing {path}")
        entry = {
            "url": f"{args.base_url.rstrip('/')}/{name}",
            "size": path.stat().st_size,
            "sha256": sha256(path),
        }
        if args.secret_key:
            entry["signature"] = sign(
                path, args.secret_key, f"foyer-desktop {args.version} {target}"
            )
        assets[target] = entry

    manifest = {
        "version": args.version,
        "notes_url": args.notes_url,
        "published_at": datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds"),
        "assets": assets,
    }
    (args.dist / "foyer-desktop.json").write_text(json.dumps(manifest, indent=2) + "\n")

    sums = [
        f"{sha256(p)}  {p.name}"
        for p in sorted(args.dist.iterdir())
        if p.is_file() and p.name not in ("SHA256SUMS", "foyer-desktop.json")
    ]
    (args.dist / "SHA256SUMS").write_text("\n".join(sums) + "\n")


if __name__ == "__main__":
    main()
