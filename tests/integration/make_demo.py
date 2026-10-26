#!/usr/bin/env python3
"""Build the README demo GIF and the social preview card from published screenshots.

  make_demo.py demo     site/img/demo.gif + demo-still.png from the manifest's screenshots
  make_demo.py social   assets/social/preview.png (+ site/img/og.png) from assets/social/preview.svg

No screen recording: every GIF frame is a screenshot already listed in
site/img/manifest.json (design_shots.capture / phase11_checks.py shots, which
replay tests/gui/design-report.json), letterboxed onto one canvas and joined
with short cross-fades by ffmpeg. Both commands record what they used in the
manifest (`demo`, `social`); check_release.py `demo` and `og` (QA26, QA29) check it.
"""
import base64, hashlib, json, os, re, shutil, struct, subprocess, sys, tempfile

ROOT = subprocess.run(["git", "rev-parse", "--show-toplevel"], capture_output=True, text=True, check=True).stdout.strip()
IMG = os.path.join(ROOT, "site/img")
MANIFEST = os.path.join(IMG, "manifest.json")
# Shot order and seconds each stays on screen, cross-fade included.
FRAMES = [("main-agent-en-light", 2.8), ("panel-en-light", 2.6), ("main-browser-en-light", 2.6),
          ("main-orphan-en-light", 2.6), ("settings-cli-en-light", 2.8)]
FADE = 0.3
WIDTH, HEIGHT = 960, 616
FPS = 10
COLOURS = 128
BACKGROUND = "0xf3ead8"
SOCIAL_SVG = "assets/social/preview.svg"
SOCIAL_PNG = "assets/social/preview.png"


def sha256(path):
    with open(path, "rb") as f:
        return hashlib.file_digest(f, "sha256").hexdigest()


def png_size(path):
    with open(path, "rb") as f:
        return struct.unpack(">II", f.read(24)[16:24])


def ffmpeg(*args):
    subprocess.run(["ffmpeg", "-v", "error", "-y", *args], check=True)


def ffmpeg_version():
    return subprocess.run(["ffmpeg", "-version"], capture_output=True, text=True, check=True).stdout.split()[2]


def demo():
    manifest = json.load(open(MANIFEST))
    images = {i["shot"]: i for i in manifest["images"]}
    missing = [shot for shot, _ in FRAMES if shot not in images]
    if missing:
        sys.exit(f"shots not in the manifest: {missing}")
    letterbox = (f"scale={WIDTH}:{HEIGHT}:force_original_aspect_ratio=decrease:flags=lanczos,"
                 f"pad={WIDTH}:{HEIGHT}:(ow-iw)/2:(oh-ih)/2:color={BACKGROUND},setsar=1")
    fit = f"{letterbox},fps={FPS},format=rgb24"
    inputs, chains = [], []
    for n, (shot, seconds) in enumerate(FRAMES):
        inputs += ["-loop", "1", "-t", str(seconds), "-i", os.path.join(IMG, images[shot]["file"])]
        chains.append(f"[{n}:v]{fit}[f{n}]")
    # xfade: each next frame starts FADE seconds before the previous one ends.
    last, offset = "f0", 0.0
    for n in range(1, len(FRAMES)):
        offset += FRAMES[n - 1][1] - FADE
        chains.append(f"[{last}][f{n}]xfade=transition=fade:duration={FADE}:offset={offset:.2f}[x{n}]")
        last = f"x{n}"
    chains.append(f"[{last}]split[a][b];[a]palettegen=max_colors={COLOURS}:stats_mode=diff[p];"
                  f"[b][p]paletteuse=dither=bayer:bayer_scale=4:diff_mode=rectangle")
    gif = os.path.join(IMG, "demo.gif")
    ffmpeg(*inputs, "-filter_complex", ";".join(chains), "-loop", "0", gif)
    # The reduced-motion still: the first frame on the same canvas.
    still = os.path.join(IMG, "demo-still.png")
    first = images[FRAMES[0][0]]
    ffmpeg("-i", os.path.join(IMG, first["file"]), "-vf", letterbox, "-frames:v", "1", still)
    frames = [{"shot": shot, "file": images[shot]["file"], "sha256": images[shot]["sha256"],
               # Seconds of the output each frame accounts for: its hold minus the overlap with the next.
               "duration_s": round(seconds - (FADE if n < len(FRAMES) - 1 else 0), 2)}
              for n, (shot, seconds) in enumerate(FRAMES)]
    probe = probe_gif(gif)
    manifest["demo"] = {"file": "demo.gif", "sha256": sha256(gif), "bytes": os.path.getsize(gif),
                        "duration_s": probe["duration"], "width": probe["width"], "height": probe["height"],
                        "fps": FPS, "fade_s": FADE, "colours": COLOURS, "ffmpeg": ffmpeg_version(), "frames": frames,
                        "still": {"file": "demo-still.png", "sha256": sha256(still), "source": first["file"],
                                  "source_sha256": first["sha256"], "width": png_size(still)[0],
                                  "height": png_size(still)[1]}}
    write(manifest)
    print(f"DEMO {gif} {probe['width']}x{probe['height']} {probe['duration']}s {os.path.getsize(gif)}B")


def probe_gif(path):
    out = subprocess.run(["ffprobe", "-v", "error", "-select_streams", "v:0", "-show_entries",
                          "stream=width,height:format=duration", "-of", "json", path],
                         capture_output=True, text=True, check=True).stdout
    data = json.loads(out)
    stream = data["streams"][0]
    return {"width": stream["width"], "height": stream["height"], "duration": float(data["format"]["duration"])}


def social():
    manifest = json.load(open(MANIFEST))
    svg = os.path.join(ROOT, SOCIAL_SVG)
    # dict.fromkeys: the same image is named by both href and xlink:href.
    embeds = list(dict.fromkeys(re.findall(r'href="\.\./\.\./site/img/([\w.-]+\.png)"', open(svg).read())))
    listed = {i["file"]: i["sha256"] for i in manifest["images"]}
    if any(name not in listed for name in embeds):
        sys.exit(f"preview.svg embeds images outside the manifest: {embeds}")
    with tempfile.TemporaryDirectory() as tmp:
        # librsvg loads no image outside the SVG's own folder: inline the screenshots in a copy.
        text = open(svg).read()
        for name in embeds:
            data = base64.b64encode(open(os.path.join(IMG, name), "rb").read()).decode()
            text = text.replace(f"../../site/img/{name}", f"data:image/png;base64,{data}")
        inlined = os.path.join(tmp, "preview.svg")
        open(inlined, "w").write(text)
        rgba = os.path.join(tmp, "preview-rgba.png")
        subprocess.run(["rsvg-convert", "-w", "1280", "-h", "640", "-o", rgba, inlined], check=True)
        # No alpha channel: GitHub and card renderers put transparent pixels on their own background.
        ffmpeg("-i", rgba, "-pix_fmt", "rgb24", os.path.join(ROOT, SOCIAL_PNG))
    og = os.path.join(IMG, "og.png")
    shutil.copyfile(os.path.join(ROOT, SOCIAL_PNG), og)
    width, height = png_size(og)
    manifest["social"] = {"source": SOCIAL_SVG, "file": SOCIAL_PNG, "og": "og.png", "sha256": sha256(og),
                          "width": width, "height": height,
                          "embeds": [{"file": name, "sha256": listed[name]} for name in embeds]}
    write(manifest)
    print(f"SOCIAL {SOCIAL_PNG} {width}x{height} {os.path.getsize(og)}B")


def write(manifest):
    with open(MANIFEST, "w") as f:
        json.dump(manifest, f, indent=2)
        f.write("\n")


if __name__ == "__main__":
    commands = {"demo": demo, "social": social}
    if len(sys.argv) != 2 or sys.argv[1] not in commands:
        sys.exit(__doc__)
    commands[sys.argv[1]]()
