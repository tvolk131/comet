#!/usr/bin/env python3
"""Package an already-built native desktop executable with cargo-packager."""
import argparse
import json
import os
from pathlib import Path
import subprocess

parser = argparse.ArgumentParser()
parser.add_argument("--target")
parser.add_argument("--debug", action="store_true")
parser.add_argument("--formats")
args = parser.parse_args()
root = Path(__file__).resolve().parents[1] / "native"
profile = "debug" if args.debug else "release"
binary_dir = root / "target"
if args.target:
    binary_dir /= args.target
binary_dir /= profile
output = root / "target" / "packages"
output.mkdir(parents=True, exist_ok=True)
config = {
    "productName": "Comet",
    "version": "0.1.0",
    "identifier": "md.comet-alpha.dev" if args.debug else "md.comet-alpha",
    "description": "Local-first Markdown notes with encrypted sync",
    "category": "Productivity",
    "authors": ["Comet contributors"],
    "binaries": [{"path": "comet", "main": True}],
    "binariesDir": str(binary_dir),
    "outDir": str(output),
    "icons": [str(root / "icons" / "icon.icns"), str(root / "icons" / "128x128.png")],
    "resources": [{"src": str(root / "licenses"), "target": "licenses"}],
}
if args.target:
    config["targetTriple"] = args.target
if os.environ.get("APPLE_SIGNING_IDENTITY"):
    config["macos"] = {"signingIdentity": os.environ["APPLE_SIGNING_IDENTITY"]}
path = output / "packager.json"
path.write_text(json.dumps(config, indent=2) + "\n")
command = ["cargo", "packager", "--config", json.dumps(config)]
if args.formats:
    command += ["--formats", args.formats]
subprocess.run(command, cwd=root, check=True)
