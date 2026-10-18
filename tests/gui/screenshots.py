#!/usr/bin/env python3
"""QA17 (Phase 05a part): panel view and close confirmation, light and dark.

Runs the bundled app with WHOATE_UI_AUTOMATION=panel-window against the live
engine (read-only), reads the window number the app writes, and captures it
with `screencapture -l<id> -x -o`. Checks: PNG exists, size equals window size
times backing scale, not a solid colour, dark is darker than light, the
window belongs to the process this script started, and the confirm shots
really have a close confirmation open (the panel shots have none). Exit 77
means macOS denied screen recording to this terminal: report
`blocked screen-recording-permission`.
"""
import json, os, struct, subprocess, sys, tempfile, time, zlib

SHOTS = [("panel", None), ("confirm", "first")]


def decode_png(path):
    data = open(path, "rb").read()
    assert data[:8] == b"\x89PNG\r\n\x1a\n", "not a PNG"
    pos, idat, header = 8, b"", None
    while pos < len(data):
        length, kind = struct.unpack(">I4s", data[pos:pos + 8])
        body = data[pos + 8:pos + 8 + length]
        if kind == b"IHDR":
            header = struct.unpack(">IIBBBBB", body)
        elif kind == b"IDAT":
            idat += body
        pos += 12 + length
    width, height, depth, colour, _, _, interlace = header
    assert depth == 8 and colour in (2, 6) and interlace == 0, f"unsupported PNG {header}"
    bpp = 4 if colour == 6 else 3
    raw, stride, rows, prev = zlib.decompress(idat), width * bpp, [], bytearray(width * bpp)
    for y in range(height):
        f, line = raw[y * (stride + 1)], bytearray(raw[y * (stride + 1) + 1:(y + 1) * (stride + 1)])
        for i in range(stride):
            a = line[i - bpp] if i >= bpp else 0
            b, c = prev[i], prev[i - bpp] if i >= bpp else 0
            if f == 1: line[i] = (line[i] + a) & 255
            elif f == 2: line[i] = (line[i] + b) & 255
            elif f == 3: line[i] = (line[i] + (a + b) // 2) & 255
            elif f == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                line[i] = (line[i] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 255
        rows.append(line)
        prev = line
    return width, height, bpp, rows


def stats(path):
    width, height, bpp, rows = decode_png(path)
    lum, colours = [], set()
    for row in rows[::4]:
        for x in range(0, width, 4):
            r, g, b = row[x * bpp:x * bpp + 3]
            lum.append(0.2126 * r + 0.7152 * g + 0.0722 * b)
            colours.add((r, g, b))
    return {"width": width, "height": height, "mean_luma": sum(lum) / len(lum), "distinct_colours": len(colours)}


def capture(binary, out_dir, name, appearance, confirm, language):
    window_file = tempfile.mktemp(prefix="whoate-window-")
    env = dict(os.environ, WHOATE_UI_AUTOMATION="panel-window", WHOATE_UI_APPEARANCE=appearance,
               WHOATE_UI_WINDOW_FILE=window_file, LC_ALL=language)
    if confirm:
        env["WHOATE_UI_CONFIRM"] = confirm
    app = subprocess.Popen([binary], env=env, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL,
                           stderr=subprocess.DEVNULL)
    try:
        for _ in range(300):
            if os.path.exists(window_file):
                break
            time.sleep(0.1)
        else:
            raise SystemExit(f"QA17_WINDOW_FILE_MISSING {name}")
        _, number, width, height, scale, pid, target = open(window_file).read().rstrip("\n").split("\t")
        if int(pid) != app.pid:
            raise SystemExit(f"QA17_WINDOW_NOT_OURS {name} pid={pid} launched={app.pid}")
        if (target != "-") != bool(confirm):
            raise SystemExit(f"QA17_CONFIRM_STATE_WRONG {name} confirm={confirm} open_for={target}")
        png = os.path.join(out_dir, f"{name}-{appearance}.png")
        shot = subprocess.run(["/usr/sbin/screencapture", f"-l{number}", "-x", "-o", png],
                              capture_output=True, text=True)
        if shot.returncode != 0 or not os.path.exists(png):
            print(f"QA17 screencapture failed: {shot.stderr.strip()}")
            sys.exit(77)
        s = stats(png)
        expected = (round(int(width) * float(scale)), round(int(height) * float(scale)))
        s.update(name=name, appearance=appearance, file=png, window=int(number), pid=app.pid, confirm_owner=target,
                 expected_size=expected, size_ok=(s["width"], s["height"]) == expected,
                 not_solid=s["distinct_colours"] > 8, source="live engine (real machine, read-only)")
        return s
    finally:
        app.terminate()
        app.wait(timeout=10)
        if os.path.exists(window_file):
            os.remove(window_file)


def main():
    binary, out_dir = sys.argv[1], sys.argv[2]
    language = sys.argv[3] if len(sys.argv) > 3 else "zh_CN.UTF-8"
    os.makedirs(out_dir, exist_ok=True)
    results = [capture(binary, out_dir, name, appearance, confirm, language)
               for name, confirm in SHOTS for appearance in ("light", "dark")]
    by = {(r["name"], r["appearance"]): r for r in results}
    ok = all(r["size_ok"] and r["not_solid"] for r in results)
    for name, _ in SHOTS:
        darker = by[(name, "dark")]["mean_luma"] < by[(name, "light")]["mean_luma"] - 40
        by[(name, "dark")]["darker_than_light"] = darker
        ok = ok and darker
    json.dump(results, open(os.path.join(out_dir, "screenshots.json"), "w"), indent=1)
    for r in results:
        print(f"QA17 {r['name']}-{r['appearance']} {r['width']}x{r['height']} size_ok={r['size_ok']} "
              f"not_solid={r['not_solid']} luma={r['mean_luma']:.0f}")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
