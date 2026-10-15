"""Fail closed if a release is missing any architecture or its checksum."""
import hashlib
import json
import os
from pathlib import Path
import tomllib
from package_release import TARGETS


def check(root, tag):
    version = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))["workspace"]["package"]["version"]
    if tag != "v" + version:
        raise ValueError(f"tag {tag} does not match Cargo version {version}")
    for target in TARGETS:
        stem = f"bmcforge-{version}-{target}"
        archive = root / "dist" / (stem + (".zip" if "windows" in target else ".tar.gz"))
        if not archive.exists():
            raise ValueError(f"missing archive for {target}: {archive.name}")
        checksum_path = archive.with_name(archive.name + ".sha256")
        if not checksum_path.exists():
            raise ValueError(f"missing checksum: {target}")
        checksum = checksum_path.read_text().split()[0]
        if hashlib.sha256(archive.read_bytes()).hexdigest() != checksum:
            raise ValueError(f"checksum mismatch: {target}")
        status_path = root / "dist" / (stem + ".build-status.json")
        if not status_path.exists():
            raise ValueError(f"missing build status: {target}")
        status = json.loads(status_path.read_text())
        if status["target"] != target or any(status[key] != "passed" for key in ["build", "unit_and_protocol_tests", "executable_smoke_test"]):
            raise ValueError(f"incomplete verification: {target}")


if __name__ == "__main__":
    check(Path(__file__).resolve().parents[1], os.environ["RELEASE_TAG"])
