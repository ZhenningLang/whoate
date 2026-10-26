#!/usr/bin/env python3
"""Retake and publish the README / product page screenshots, demo data only.

  publish_shots.py <debug Whoate> <whoate CLI> <raw out dir>

Main window and panel shots: design_shots.capture (tests/gui/design_shots.py),
in English. Settings window: `tests/gui/phase11_checks.py shots`. Both replay
tests/gui/design-report.json through launch.py (`open -n`, isolated); nothing
of the real machine is shown. Raw captures go to <raw out dir>; the published
ones are copied to site/img/<name>.png (scaled down with sips to at most
MAX_WIDTH pixels) and recorded in site/img/manifest.json: generator, shot name,
sha256 of the published and the source PNG, the app/ tree and the demo report.
check_release.py `shots` (QA15) compares that against the tree being checked,
so a changed interface turns it red until reshot. Keeps the manifest's `demo`
section only when its frames are unchanged (make_demo.py rewrites it).
Exit 77: screen recording denied (report `blocked screen-recording-permission`).
"""
import glob, hashlib, json, os, shutil, struct, subprocess, sys, tempfile

ROOT = subprocess.run(["git", "rev-parse", "--show-toplevel"], capture_output=True, text=True, check=True).stdout.strip()
sys.path.insert(0, os.path.join(ROOT, "tests/gui"))
import design_shots  # noqa: E402

IMG = os.path.join(ROOT, "site/img")
FIXTURE = "tests/gui/design-report.json"
MAX_WIDTH = 1600
CAPTURE = "design_shots.capture"
SETTINGS = "phase11_checks.py shots"
# (shot, appearance) from design_shots.SHOTS, all in English.
MAIN = [("panel", "light"), ("main-agent", "light"), ("main-browser", "light"), ("main-orphan", "light"),
        ("main-agent", "dark")]
# Files from one phase11_checks.py shots run.
SETTINGS_FILES = ["settings-cli-en-light.png"]


def sha256(path):
    with open(path, "rb") as f:
        return hashlib.file_digest(f, "sha256").hexdigest()


def size(path):
    """Width and height from the PNG IHDR chunk."""
    with open(path, "rb") as f:
        return struct.unpack(">II", f.read(24)[16:24])


def capture_main(binary, raw):
    fixture = os.path.join(tempfile.mkdtemp(prefix="whoate-publish-"), "design-report.json")
    design_shots.fresh_fixture(fixture)
    shots = {s[0]: s for s in design_shots.SHOTS}
    taken = []
    for name, appearance in MAIN:
        r = design_shots.capture(binary, raw, fixture, shots[name], appearance, design_shots.EN)
        test = design_shots.SELECTED.get(name)
        good = r["not_solid"] and r["size_ok"] and (test is None or (r["selected"] is not None and test(r["selected"])))
        print(f"SHOT {r['file']} not_solid={r['not_solid']} size_ok={r['size_ok']} selected={r['selected']}")
        if not good:
            sys.exit(f"capture of {r['file']} failed its checks")
        taken.append((r["file"], CAPTURE))
    return taken


def capture_settings(binary, cli, raw):
    out = os.path.join(raw, "settings")
    os.makedirs(out, exist_ok=True)
    run = subprocess.run([sys.executable, "-B", os.path.join(ROOT, "tests/gui/phase11_checks.py"), "shots",
                          binary, out, cli], capture_output=True, text=True)
    print(run.stdout[-2000:] + run.stderr[-2000:])
    if run.returncode == 77:
        sys.exit(77)
    receipt = json.load(open(os.path.join(out, "qa22-screenshots.json")))
    if not receipt["app_env_home_free"]:
        sys.exit("settings shots: the app was given a path in the owner's home")
    by_file = {os.path.basename(s["file"]): s for s in receipt["shots"]}
    for name in SETTINGS_FILES:
        if not by_file.get(name, {}).get("ok"):
            sys.exit(f"settings shot {name} failed: {by_file.get(name)}")
        shutil.copyfile(os.path.join(out, name), os.path.join(raw, name))
    return [(name, SETTINGS) for name in SETTINGS_FILES]


def publish(raw, taken):
    os.makedirs(IMG, exist_ok=True)
    published = {name for name, _ in taken}
    old = json.load(open(os.path.join(IMG, "manifest.json"))) if os.path.exists(os.path.join(IMG, "manifest.json")) else {}
    # Earlier screenshots that are no longer published; make_demo.py and the social card own the rest.
    for stale in glob.glob(os.path.join(IMG, "*.png")):
        name = os.path.basename(stale)
        if name not in published and name not in ("demo-still.png", "og.png"):
            os.remove(stale)
    images = []
    for name, generator in taken:
        source, target = os.path.join(raw, name), os.path.join(IMG, name)
        shutil.copyfile(source, target)
        if size(target)[0] > MAX_WIDTH:
            subprocess.run(["sips", "--resampleWidth", str(MAX_WIDTH), target], capture_output=True, check=True)
        width, height = size(target)
        images.append({"file": name, "sha256": sha256(target), "shot": name.rsplit(".", 1)[0], "generator": generator,
                       "source_sha256": sha256(source), "width": width, "height": height})
    app_tree = subprocess.run(["git", "rev-parse", "HEAD:app"], cwd=ROOT, capture_output=True, text=True,
                              check=True).stdout.strip()
    manifest = {"fixture": FIXTURE, "fixture_sha256": sha256(os.path.join(ROOT, FIXTURE)), "app_tree": app_tree,
                "images": images}
    shas = {i["file"]: i["sha256"] for i in images}
    if old.get("demo") and all(shas.get(f["file"]) == f["sha256"] for f in old["demo"]["frames"]):
        manifest["demo"] = old["demo"]
    if "social" in old:
        manifest["social"] = old["social"]
    with open(os.path.join(IMG, "manifest.json"), "w") as f:
        json.dump(manifest, f, indent=2)
        f.write("\n")
    for image in images:
        print(f"PUBLISHED {image['file']} {image['width']}x{image['height']} {os.path.getsize(os.path.join(IMG, image['file']))}B")


def main(binary, cli, raw):
    os.makedirs(raw, exist_ok=True)
    publish(raw, capture_main(binary, raw) + capture_settings(binary, cli, raw))


if __name__ == "__main__":
    if len(sys.argv) != 4:
        sys.exit(__doc__)
    main(*sys.argv[1:])
