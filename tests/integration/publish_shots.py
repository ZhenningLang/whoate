#!/usr/bin/env python3
"""Copy the README / product page screenshots out of a design_shots.py run.

  publish_shots.py <design_shots out dir>

Writes site/img/<shot>.png (scaled down with sips to at most MAX_WIDTH pixels)
and site/img/manifest.json, which records where every image came from: the
shot name, the source PNG's sha256, the app/ tree and the demo report the
shots replayed. check_release.py `shots` (QA15) compares that against the
tree being checked, so a changed interface turns it red until reshot.
Only demo data (tests/gui/design-report.json) is ever published.
"""
import hashlib, json, os, shutil, struct, subprocess, sys

ROOT = subprocess.run(["git", "rev-parse", "--show-toplevel"], capture_output=True, text=True, check=True).stdout.strip()
IMG = os.path.join(ROOT, "site/img")
FIXTURE = "tests/gui/design-report.json"
MAX_WIDTH = 1600
# design_shots.py output files, published under the same names.
SHOTS = ("main-agent-en-light.png", "panel-en-light.png", "main-browser-dark.png")


def sha256(path):
    with open(path, "rb") as f:
        return hashlib.file_digest(f, "sha256").hexdigest()


def size(path):
    """Width and height from the PNG IHDR chunk."""
    with open(path, "rb") as f:
        return struct.unpack(">II", f.read(24)[16:24])


def main(src):
    os.makedirs(IMG, exist_ok=True)
    images = []
    for name in SHOTS:
        source = os.path.join(src, name)
        target = os.path.join(IMG, name)
        shutil.copyfile(source, target)
        if size(target)[0] > MAX_WIDTH:
            subprocess.run(["sips", "--resampleWidth", str(MAX_WIDTH), target], capture_output=True, check=True)
        width, height = size(target)
        images.append({"file": name, "sha256": sha256(target), "shot": name.rsplit(".", 1)[0],
                       "source_sha256": sha256(source), "width": width, "height": height})
    app_tree = subprocess.run(["git", "rev-parse", "HEAD:app"], cwd=ROOT, capture_output=True, text=True,
                              check=True).stdout.strip()
    manifest = {"generator": "tests/gui/design_shots.py --strict", "fixture": FIXTURE,
                "fixture_sha256": sha256(os.path.join(ROOT, FIXTURE)), "app_tree": app_tree, "images": images}
    with open(os.path.join(IMG, "manifest.json"), "w") as f:
        json.dump(manifest, f, indent=2)
        f.write("\n")
    for image in images:
        print(f"SHOT {image['file']} {image['width']}x{image['height']} {os.path.getsize(os.path.join(IMG, image['file']))}B")


if __name__ == "__main__":
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    main(sys.argv[1])
