#!/usr/bin/env python3
"""Phase 06 (release) checks. One subcommand per check; each prints
`ok ...` lines and exits 0, or prints `FAIL ...` and exits 1.

  ci                       .github/workflows/ci.yml structure (QA3)
  pages                    .github/workflows/pages.yml structure (QA13)
  history <ref> <out dir>  frozen-contract baselines are ancestors of <ref>; shallow clone negative control (QA5)
  templates                Homebrew templates hold placeholders only (QA9 fast)
  rendered <release dir>   rendered formula and cask match the built assets (QA9 full)
  style <release dir>      brew style on the rendered formula and cask (QA9 full)
  release <release dir>    asset contents, architecture, signature, Info.plist (QA6)
  readme <whoate>          README content, links, subcommands, rules example (QA10)
  site                     product page static checks (QA11)
  render <out dir>         product page in agent-browser, 390x844 and 1440x900, light and dark (QA12)
  logo <base> [<end>]      logo sources, icon set, references (QA14)
  shots [<ref>]            site/img/manifest.json matches images, app tree and demo report (QA15)
  insights <whoate>        `insights --list` equals every catalog entry (QA18)

Run from the repository root (verify.sh does). Never writes outside its temp dirs.
"""
import glob, hashlib, html.parser, json, os, plistlib, re, subprocess, sys, tempfile, tomllib

INSTALL = "brew install zhenninglang/tap/whoate"
CASK = "brew install --cask zhenninglang/tap/whoate"
PAGES = "https://zhenninglang.github.io/whoate/"
REPO = "https://github.com/ZhenningLang/whoate"
BUNDLE_ID = "io.github.zhenninglang.whoate"
TAP_DOCS = ["README.md", "docs/install.md", "site/index.html", *sorted(glob.glob("packaging/**/*", recursive=True))]

failures = []


def ok(msg):
    print(f"ok {msg}")


def check(cond, msg):
    if cond:
        ok(msg)
    else:
        print(f"FAIL {msg}")
        failures.append(msg)
    return cond


def run(*cmd, timeout=600, **kw):
    """A child that hangs (browser daemon, brew) fails its check instead of blocking verify."""
    try:
        return subprocess.run(cmd, capture_output=True, text=True, timeout=timeout, **kw)
    except subprocess.TimeoutExpired:
        return subprocess.CompletedProcess(cmd, 124, "", f"timeout after {timeout}s: {' '.join(cmd)}")


def cargo_version():
    return tomllib.load(open("Cargo.toml", "rb"))["workspace"]["package"]["version"]


def load_yaml(path):
    import yaml
    doc = yaml.safe_load(open(path))
    # PyYAML reads the bare key `on` as True.
    return doc, doc.get("on", doc.get(True))


def steps_of(doc):
    return [s for job in doc["jobs"].values() for s in job.get("steps", [])]


def ci():
    path = ".github/workflows/ci.yml"
    text = open(path).read()
    doc, on = load_yaml(path)
    check(on["push"]["branches"] == ["main"], "ci push main")
    check(on["pull_request"]["branches"] == ["main"], "ci pull_request main")
    check(doc.get("permissions") == {"contents": "read"}, "ci permissions contents: read")
    check("secrets." not in text, "ci uses no secrets")
    for name, job in doc["jobs"].items():
        runs_on = job["runs-on"]
        check(re.fullmatch(r"macos-\d+(-arm64)?", str(runs_on)) is not None and "latest" not in str(runs_on),
              f"ci job {name} runs on an explicit Apple Silicon image ({runs_on})")
    steps = steps_of(doc)
    checkout = [s for s in steps if str(s.get("uses", "")).startswith("actions/checkout@")]
    check(checkout and checkout[0].get("with", {}).get("fetch-depth") == 0, "ci checkout fetch-depth 0")
    runs = [s.get("run", "") for s in steps]
    joined = "\n".join(runs)
    check(re.search(r"rustup toolchain install 1\.96\.0\b.*clippy", joined) is not None
          and "rustup default 1.96.0" in joined, "ci pins Rust 1.96.0 with clippy")
    lines = [line.strip() for r in runs for line in r.splitlines() if line.strip()]

    def index(pred, what):
        found = [i for i, line in enumerate(lines) if pred(line)]
        check(len(found) >= 1, f"ci runs {what}")
        return found[0] if found else -1

    test = index(lambda l: l.startswith("cargo test --workspace --locked") and "--all-features" not in l,
                 "cargo test --workspace --locked (no --all-features)")
    check(not any(l.startswith("cargo test") and "--all-features" in l for l in lines),
          "ci cargo test never uses --all-features")
    clippy = index(lambda l: l.startswith("cargo clippy") and "--workspace" in l and l.endswith("-D warnings"),
                   "cargo clippy --workspace ... -D warnings")
    ffi = index(lambda l: l.startswith("cargo build -p whoate-ffi"), "cargo build -p whoate-ffi")
    swift = index(lambda l: l.startswith("swift build --package-path app"), "swift build --package-path app")
    check(-1 not in (test, clippy, ffi, swift) and ffi < swift, "ci builds whoate-ffi before swift build")
    check(any("swift --version" in l for l in lines) and any("rustc --version" in l for l in lines),
          "ci prints toolchain versions")


def ci_commands():
    """The CI run lines in order, for the clean-clone replay (QA4)."""
    doc, _ = load_yaml(".github/workflows/ci.yml")
    for s in steps_of(doc):
        if "run" in s and not s["run"].lstrip().startswith(("sw_vers", "rustup")):
            print(s["run"].strip().replace("\n", " && "))


def pages():
    path = ".github/workflows/pages.yml"
    text = open(path).read()
    doc, on = load_yaml(path)
    check(on["push"]["branches"] == ["main"] and "site/**" in on["push"].get("paths", []), "pages push main, paths site/**")
    check("workflow_dispatch" in on, "pages workflow_dispatch")
    perms = doc.get("permissions", {})
    check(perms.get("pages") == "write" and perms.get("id-token") == "write", "pages permissions pages/id-token write")
    check("concurrency" in doc, "pages concurrency group")
    check("secrets." not in text, "pages uses no secrets")
    uses = [str(s.get("uses", "")) for s in steps_of(doc)]
    order = [next((i for i, u in enumerate(uses) if u.startswith(f"actions/{a}@")), -1)
             for a in ("configure-pages", "upload-pages-artifact", "deploy-pages")]
    check(-1 not in order and order == sorted(order), "pages configure -> upload -> deploy")
    upload = [s for s in steps_of(doc) if str(s.get("uses", "")).startswith("actions/upload-pages-artifact@")]
    check(upload and upload[0].get("with", {}).get("path") == "site", "pages uploads path site")


def baseline_refs():
    """Every 40-hex string literal in the Rust sources: the frozen-contract baselines."""
    shas = set()
    for path in glob.glob("crates/**/*.rs", recursive=True):
        shas.update(re.findall(r'"([0-9a-f]{40})(?![0-9a-f])', open(path).read()))
    return sorted(shas)


def baseline_failures(shas, ref, repo="."):
    """A reference fails if it is missing as a commit or is not an ancestor of ref."""
    failed = []
    for sha in shas:
        if run("git", "-C", repo, "cat-file", "-e", sha + "^{commit}").returncode != 0:
            failed.append(f"{sha[:12]} missing")
        elif run("git", "-C", repo, "merge-base", "--is-ancestor", sha, ref).returncode != 0:
            failed.append(f"{sha[:12]} not an ancestor of {ref}")
    return failed


def history(ref, out):
    shas = baseline_refs()
    check(len(shas) >= 4, f"history found {len(shas)} baseline references: {[s[:7] for s in shas]}")
    failed = baseline_failures(shas, ref)
    check(not failed, f"history every baseline is a commit and an ancestor of {ref} {failed}")
    # Negative control 1: a referenced sha that does not exist must fail, not be skipped.
    missing = "f" * 40
    caught = baseline_failures([*shas, missing], ref)
    check(caught == [f"{missing[:12]} missing"], f"history negative control: missing reference is caught {caught}")
    # Negative control 2: a depth-1 clone lacks the baselines (why CI needs fetch-depth 0).
    shallow = os.path.join(out, "shallow")
    url = "file://" + os.path.abspath(".")
    clone = run("git", "clone", "-q", "--depth", "1", "--no-local", url, shallow)
    if not check(clone.returncode == 0, "history shallow clone"):
        print(clone.stderr)
        return
    lacking = [f for f in baseline_failures(shas, "HEAD", shallow) if f.endswith("missing")]
    check(len(lacking) >= 1, f"history negative control: depth 1 clone lacks {len(lacking)} baseline(s)")


def templates():
    for kind in ("Formula", "Casks"):
        path = f"packaging/homebrew-tap/{kind}/whoate.rb"
        text = open(path).read()
        check("@VERSION@" in text, f"{path} has @VERSION@")
        check(("@CLI_SHA256@" if kind == "Formula" else "@APP_SHA256@") in text, f"{path} has its sha placeholder")
        check(re.search(r"\b[0-9a-f]{64}\b", text) is None, f"{path} has no stale sha256")
        check(run("ruby", "-c", path).returncode == 0, f"{path} ruby -c")


def assets(version):
    return f"whoate-v{version}-aarch64-apple-darwin.tar.gz", f"whoate-v{version}-macos-arm64-app.zip"


def rendered(out):
    version = cargo_version()
    cli, app = assets(version)
    sums = dict(reversed(line.split()) for line in open(os.path.join(out, "SHA256SUMS.txt")))
    check(set(sums) == {cli, app}, f"rendered SHA256SUMS lists {sorted(sums)}")
    formula = open(os.path.join(out, "homebrew-tap/Formula/whoate.rb")).read()
    cask = open(os.path.join(out, "homebrew-tap/Casks/whoate.rb")).read()
    base = f"{REPO}/releases/download/v{version}/"
    for name, text in (("formula", formula), ("cask", cask)):
        check(re.search(r"@[A-Z0-9_]+@", text) is None, f"rendered {name} has no placeholder")
        check(run("ruby", "-c", input=text).returncode == 0, f"rendered {name} ruby -c")
    url = re.search(r'^\s*url "([^"]+)"', formula, re.M).group(1)
    check(url == base + cli and os.path.exists(os.path.join(out, cli)), f"formula url {url}")
    check(re.search(rf'^\s*sha256 "{sums.get(cli)}"', formula, re.M) is not None, "formula sha256 = SHA256SUMS")
    for stanza in ('bin.install "whoate"', "depends_on arch: :arm64", "depends_on macos: :sonoma",
                   'pkgshare.install "schema/whoate-v1.schema.json"', "skills/whoate/SKILL.md", "test do",
                   'license "MIT"', f'homepage "{PAGES}"'):
        check(stanza in formula, f"formula has {stanza}")
    cask_version = re.search(r'^\s*version "([^"]+)"', cask, re.M).group(1)
    check(cask_version == version, f"cask version {cask_version}")
    cask_url = re.search(r'^\s*url "([^"]+)"', cask, re.M).group(1).replace("#{version}", cask_version)
    check(cask_url == base + app and os.path.exists(os.path.join(out, app)), f"cask url {cask_url}")
    check(re.search(rf'^\s*sha256 "{sums.get(app)}"', cask, re.M) is not None, "cask sha256 = SHA256SUMS")
    for stanza in ('app "whoate.app"', "depends_on arch: :arm64", "depends_on macos: :sonoma",
                   "Open Anyway", "仍要打开", "xattr -d com.apple.quarantine"):
        check(stanza in cask, f"cask has {stanza}")


def style(out):
    """brew style applies formula/cask cops only to files under Taps/<user>/homebrew-<tap>/."""
    with tempfile.TemporaryDirectory() as tmp:
        tap = os.path.join(tmp, "Library/Taps/zhenninglang/homebrew-tap")
        files = []
        for kind in ("Formula", "Casks"):
            os.makedirs(os.path.join(tap, kind))
            files.append(os.path.join(tap, kind, "whoate.rb"))
            with open(files[-1], "w") as f:
                f.write(open(os.path.join(out, "homebrew-tap", kind, "whoate.rb")).read())
        result = run("brew", "style", *files)
        print(result.stdout + result.stderr)
        check(result.returncode == 0, "brew style (formula and cask cops) passes")


def release(out):
    # release.sh checks the same assets itself (its own gate when run by hand);
    # this is the independent check of what it produced, plus contents it does not check.
    version = cargo_version()
    cli, app = assets(version)
    for name in (cli, app, "SHA256SUMS.txt", "BUILD_INFO.txt", "homebrew-tap/Formula/whoate.rb",
                 "homebrew-tap/Casks/whoate.rb", "homebrew-tap/README.md"):
        check(os.path.isfile(os.path.join(out, name)), f"release has {name}")
    check(run("shasum", "-a", "256", "-c", "SHA256SUMS.txt", cwd=out).returncode == 0, "release shasum -c")
    listing = run("tar", "-tzf", os.path.join(out, cli)).stdout.split()
    top = f"whoate-v{version}-aarch64-apple-darwin/"
    for member in ("whoate", "LICENSE", "README.md", "schema/whoate-v1.schema.json", "skills/whoate/SKILL.md"):
        check(top + member in listing, f"tarball has {member}")
    with tempfile.TemporaryDirectory() as tmp:
        run("tar", "-C", tmp, "-xzf", os.path.join(out, cli))
        binary = os.path.join(tmp, top, "whoate")
        check(run("lipo", "-archs", binary).stdout.strip() == "arm64", "CLI lipo -archs = arm64")
        build = run("vtool", "-show-build", binary).stdout
        check(re.search(r"^\s*minos 14\.0$", build, re.M) is not None, "CLI minos 14.0")
        check(run(binary, "--version").stdout.strip() == f"whoate {version}", f"CLI --version = whoate {version}")
        for member in ("schema/whoate-v1.schema.json", "skills/whoate/SKILL.md", "LICENSE"):
            check(open(os.path.join(tmp, top, member), "rb").read() == open(member, "rb").read(), f"tarball {member} = repo")
        check(run("ditto", "-x", "-k", os.path.join(out, app), tmp).returncode == 0, "app zip unpacks with ditto")
        bundle = os.path.join(tmp, "whoate.app")
        verify = run("codesign", "--verify", "--strict", bundle)
        check(verify.returncode == 0, f"app codesign --verify --strict {verify.stderr.strip()}")
        symbols = run("nm", os.path.join(bundle, "Contents/MacOS/Whoate"))
        check(symbols.returncode == 0 and "_whoate_engine_new_from_fixture" not in symbols.stdout,
              "app has no _whoate_engine_new_from_fixture")
        plist = plistlib.load(open(os.path.join(bundle, "Contents/Info.plist"), "rb"))
        check(plist.get("CFBundleShortVersionString") == version, f"app version {plist.get('CFBundleShortVersionString')}")
        check(plist.get("CFBundleIdentifier") == BUNDLE_ID, f"app bundle id {plist.get('CFBundleIdentifier')}")
        check(plist.get("LSMinimumSystemVersion") == "14.0", "app LSMinimumSystemVersion 14.0")
        for res in ("AppIcon.icns", "menubar-template.pdf"):
            check(os.path.isfile(os.path.join(bundle, "Contents/Resources", res)), f"app has {res}")


def fenced(text, lang=None):
    pattern = r"```" + (re.escape(lang) if lang else r"[a-z]*") + r"\n(.*?)```"
    return re.findall(pattern, text, re.S)


def local_targets(path, text):
    """Relative link and image targets in a Markdown or HTML file."""
    found = re.findall(r"\]\(([^)\s]+)\)", text) + re.findall(r'(?:src|href)="([^"]+)"', text)
    base = os.path.dirname(path)
    for target in found:
        if re.match(r"^(https?:|mailto:|#)", target):
            continue
        yield target, os.path.normpath(os.path.join(base, target.split("#")[0]))


def help_commands(whoate):
    out = run(whoate, "--help").stdout
    block = out.split("Commands:")[1].split("Options:")[0]
    return {line.split()[0] for line in block.splitlines() if line.strip()}


def top_issues(whoate, rules, tmp):
    env = dict(os.environ, WHOATE_RULES=rules, WHOATE_CACHE_DIR=os.path.join(tmp, "cache"),
               WHOATE_HOME_OVERRIDE=os.path.join(tmp, "home"))
    os.makedirs(env["WHOATE_HOME_OVERRIDE"], exist_ok=True)
    proc = run(whoate, "top", "--json", "-n", "1", env=env)
    report = json.loads(proc.stdout)
    # Exit 1 is also an "act" diagnosis; only an error object makes it fatal.
    ok_run = proc.returncode in (0, 1) and "error" not in report and "issues" in report
    return ok_run, [i for i in report.get("issues", []) if i.get("stage") == "user_rules"]


def readme(whoate):
    text = open("README.md").read()
    check(INSTALL in text, f"README has `{INSTALL}`")
    check(CASK in text, f"README has `{CASK}`")
    for word in ("系统设置", "隐私与安全性", "仍要打开", "Open Anyway", "xattr -d com.apple.quarantine"):
        check(word in text, f"README first-launch step mentions {word}")
    check(re.search(r"^#+ .*[一-鿿]", text, re.M) is not None, "README has a Chinese section heading")
    check(PAGES in text, "README links the product page")
    check("docs/design.md" not in text, "README no longer links docs/design.md")
    for path in ("README.md", "docs/install.md"):
        for target, resolved in local_targets(path, open(path).read()):
            check(os.path.exists(resolved), f"{path} link {target} exists")
    # Every tap spelling in user-facing files is zhenninglang/tap.
    for path in TAP_DOCS:
        if not os.path.isfile(path):
            continue
        body = open(path).read()
        for spelling in re.findall(r"[\w.-]+/(?:homebrew-)?tap/whoate\b", body):
            check(spelling == "zhenninglang/tap/whoate", f"{path} tap spelling {spelling}")
        for arg in re.findall(r"\bbrew (?:un)?tap ([\w./-]+)", body):
            check(arg == "zhenninglang/tap", f"{path} brew tap argument {arg}")
    commands = help_commands(whoate)
    used = set()
    for block in fenced(text, "sh"):
        used.update(re.findall(r"^\s*(?:[^#\n]*?&&\s*)?whoate ([a-z][a-z-]*)", block, re.M))
        used.update(re.findall(r"\bwhoate ([a-z][a-z-]*)\b(?=[^\n]*>)", block))
    used.update(re.findall(r"`whoate ([a-z][a-z-]*)", text))
    check(len(used) >= 8, f"README shows {len(used)} subcommands")
    for sub in sorted(used):
        check(sub in commands, f"README subcommand `whoate {sub}` is in --help")
    examples = fenced(text, "toml")
    if not check(len(examples) == 1, "README has one toml rules example"):
        return
    with tempfile.TemporaryDirectory() as tmp:
        good = os.path.join(tmp, "rules.toml")
        open(good, "w").write(examples[0])
        rules = tomllib.loads(examples[0])["rules"]
        check(all(set(r) <= {"executable", "argv_regex", "cwd_prefix", "label", "advice"} for r in rules),
              "README rules use only loader fields")
        ran, issues = top_issues(whoate, good, tmp)
        check(ran and issues == [], f"README rules example loads (report ok {ran}, user_rules issues {issues})")
        bad = os.path.join(tmp, "bad.toml")
        open(bad, "w").write(examples[0].replace("[[rules]]\n", "[[rules]]\nprocess = \"node\"\n", 1))
        ran, issues = top_issues(whoate, bad, tmp)
        check(ran and len(issues) == 1, f"negative control: unknown field gives a user_rules issue ({issues})")


class Page(html.parser.HTMLParser):
    def __init__(self):
        super().__init__()
        self.tags, self.attrs = [], []
        self.text = []

    def handle_starttag(self, tag, attrs):
        self.tags.append(tag)
        self.attrs.append((tag, dict(attrs)))

    def handle_data(self, data):
        self.text.append(data)


def site():
    path = "site/index.html"
    text = open(path).read()
    page = Page()
    page.feed(text)
    page.close()
    attrs = page.attrs
    html_tag = [a for t, a in attrs if t == "html"]
    check(html_tag and html_tag[0].get("lang"), "site <html lang>")
    metas = [a for t, a in attrs if t == "meta"]
    check(any(m.get("name") == "viewport" for m in metas), "site viewport meta")
    check(any(m.get("name") == "description" and len(m.get("content", "")) > 40 for m in metas), "site meta description")
    check("title" in page.tags, "site <title>")
    check("prefers-color-scheme: dark" in text, "site dark color scheme rules")
    tidy = run("tidy", "-errors", "-q", "-utf8", "--new-blocklevel-tags",
               "header,nav,main,section,figure,figcaption,footer", "--new-inline-tags", "kbd", path)
    errors = [l for l in tidy.stderr.splitlines() if "Error:" in l]
    check(tidy.returncode in (0, 1) and not errors, f"site tidy reports no errors {errors[:3]}")
    scripts = [a for t, a in attrs if t == "script"]
    check(not any(a.get("src") for a in scripts), "site loads no external script")
    links = [a for t, a in attrs if t == "link"]
    check(not any(re.match(r"https?:", a.get("href", "")) for a in links), "site links no external stylesheet or font")
    check(re.search(r"@import|@font-face|fonts\.googleapis|googletagmanager|google-analytics|plausible|"
                    r"umami|segment\.|hotjar|clarity\.ms", text) is None, "site has no web fonts or trackers")
    for target, resolved in local_targets(path, text):
        check(resolved.startswith("site") and os.path.exists(resolved), f"site resource {target} exists inside site/")
    imgs = [a for t, a in attrs if t == "img"]
    # The header logo beside the "whoate" text is decorative: empty alt (spec ruling 23:3x).
    check(all("alt" in a for a in imgs) and all(a["alt"].strip() for a in imgs if "favicon" not in a.get("src", "")),
          f"site {len(imgs)} images carry alt text (decorative brand logo: empty alt)")
    body = " ".join(page.text)
    check(re.search(r"per-software", body, re.I) and "按软件" in body, "site states per-software insight (en, zh)")
    check(re.search(r"zero tokens", body, re.I) and "零 token" in body, "site states zero tokens (en, zh)")
    commands = re.findall(r"<code>(brew install [^<]+)</code>", text)
    check(INSTALL in commands and all(c in open("README.md").read() for c in commands),
          f"site install commands match README {sorted(set(commands))}")
    check(f'href="{REPO}"' in text and f'href="{REPO}/releases"' in text, "site links repository and Releases")
    total = sum(os.path.getsize(f) for f in glob.glob("site/**/*", recursive=True) if os.path.isfile(f))
    check(total <= 5 * 1024 * 1024, f"site/ is {total} bytes (<= 5MB)")
    for f in glob.glob("site/img/*.png"):
        check(os.path.getsize(f) <= 600 * 1024, f"{f} <= 600KB")


def render(out):
    """Open site/index.html in agent-browser at phone and desktop size, light and dark (QA12)."""
    session = f"whoate-p06-{os.getpid()}"
    url = "file://" + os.path.abspath("site/index.html")
    probe = ("JSON.stringify({sw: document.documentElement.scrollWidth, iw: window.innerWidth, "
             "imgs: [...document.images].map(i => [i.getAttribute('src'), i.complete && i.naturalWidth > 0]), "
             "dark: matchMedia('(prefers-color-scheme: dark)').matches})")

    def ab(*args):
        return run("agent-browser", "--session", session, *args)

    try:
        first = True
        for width, height in ((390, 844), (1440, 900)):
            for scheme in ("light", "dark"):
                tag = f"{width}x{height}-{scheme}"
                ab("set", "viewport", str(width), str(height))
                ab("set", "media", scheme)
                nav = ab("open", url) if first else ab("reload")
                if not check(nav.returncode == 0, f"render {tag} page opens"):
                    print(nav.stdout + nav.stderr)
                    return
                first = False
                raw = ab("eval", probe).stdout.strip().splitlines()[-1]
                state = json.loads(json.loads(raw))
                check(state["dark"] == (scheme == "dark"), f"render {tag} color scheme applied")
                check(state["iw"] == width and state["sw"] <= state["iw"],
                      f"render {tag} no horizontal overflow (scrollWidth {state['sw']}, innerWidth {state['iw']})")
                broken = [src for src, loaded in state["imgs"] if not loaded]
                check(state["imgs"] and not broken, f"render {tag} {len(state['imgs'])} images loaded {broken}")
                shot = ab("screenshot", "--full", os.path.join(out, f"site-{tag}.png"))
                check(shot.returncode == 0, f"render {tag} screenshot saved")
        errors = ab("errors").stdout.strip()
        console = [l for l in ab("console").stdout.splitlines() if "error" in l.lower()]
        check(not errors and not console, f"render no page or console errors {errors!r} {console}")
    finally:
        ab("close")


def logo(base, end=None):
    import xml.etree.ElementTree as ET
    svgs = sorted(glob.glob("assets/logo/*.svg"))
    check(len(svgs) == 3, f"logo sources {svgs}")
    for svg in svgs:
        try:
            root = ET.parse(svg).getroot()
            check(root.get("viewBox") is not None, f"{svg} is XML with viewBox")
        except ET.ParseError as e:
            check(False, f"{svg} parses ({e})")
    with tempfile.TemporaryDirectory() as tmp:
        iconset = os.path.join(tmp, "AppIcon.iconset")
        check(run("iconutil", "-c", "iconset", "assets/logo/AppIcon.icns", "-o", iconset).returncode == 0, "iconutil unpacks AppIcon.icns")
        pngs = sorted(os.listdir(iconset)) if os.path.isdir(iconset) else []
        want = sorted(f"icon_{s}x{s}{x}.png" for s in (16, 32, 128, 256, 512) for x in ("", "@2x"))
        check(pngs == want, f"AppIcon.icns has the 10 sizes ({len(pngs)})")
    check(os.path.getsize("assets/logo/menubar-template.pdf") > 0, "menubar-template.pdf is not empty")
    diff = ["git", "diff", "--name-only", base, *([end] if end else []), "--", "assets/logo"]
    changed = set(run(*diff).stdout.split())
    if any(c.endswith(".svg") for c in changed):
        check({"assets/logo/AppIcon.icns", "assets/logo/menubar-template.pdf"} <= changed,
              f"svg changed, icns and pdf regenerated too ({sorted(changed)})")
    else:
        ok(f"logo svg unchanged this phase ({sorted(changed)})")
    check(open("site/favicon.svg", "rb").read() == open("assets/logo/logo.svg", "rb").read(),
          "site/favicon.svg is assets/logo/logo.svg")
    check('src="assets/logo/logo.svg"' in open("README.md").read(), "README header uses assets/logo/logo.svg")
    check('href="favicon.svg"' in open("site/index.html").read(), "site favicon is favicon.svg")


def sha256(path):
    with open(path, "rb") as f:
        return hashlib.file_digest(f, "sha256").hexdigest()


def shots(ref=None):
    manifest = json.load(open("site/img/manifest.json"))
    source = open("tests/gui/design_shots.py").read()
    names = set(re.findall(r'^\s*\("([a-z-]+)", "(?:gallery|panel|main)"', source, re.M))
    check(names, f"design_shots SHOTS {sorted(names)}")
    for image in manifest["images"]:
        path = os.path.join("site/img", image["file"])
        check(os.path.isfile(path) and sha256(path) == image["sha256"], f"{path} sha256 matches manifest")
        shot = re.sub(r"(-en)?-(light|dark)$", "", image["shot"])
        check(shot in names, f"{image['file']} comes from shot {shot}")
    tree = run("git", "rev-parse", f"{ref or 'HEAD'}:app").stdout.strip()
    if ref is None:
        dirty = run("git", "status", "--porcelain", "--", "app").stdout.strip()
        check(not dirty, "app/ has no uncommitted changes")
    check(manifest["app_tree"] == tree, f"screenshots taken on app tree {manifest['app_tree'][:12]}, checked tree {tree[:12]}")
    check(manifest["fixture_sha256"] == sha256("tests/gui/design-report.json"), "screenshots replay the current demo report")
    listed = {image["file"] for image in manifest["images"]}
    for path in ("README.md", "site/index.html"):
        for img in re.findall(r"img/([\w.-]+\.png)", open(path).read()):
            check(img in listed, f"{path} image {img} is in the manifest")
    check(sorted(os.path.basename(p) for p in glob.glob("site/img/*.png")) == sorted(listed),
          "site/img holds only manifest images")


def insights(whoate):
    out = run(whoate, "insights", "--list")
    check(out.returncode == 0, "insights --list exits 0")
    # Four lines per entry: id, cause, advice, "<source> <checked>".
    blocks = re.findall(r"^(\S+)\n  .*\n  .*\n  (.+) (\S+)$", out.stdout, re.M)
    listed = {b[0]: b for b in blocks}
    catalog = {e["id"] for path in glob.glob("crates/whoate-core/src/insights/*.toml")
               for e in tomllib.load(open(path, "rb"))["entries"]}
    check(len(catalog) >= 35, f"catalog has {len(catalog)} entries")
    check(set(listed) == catalog, f"insights --list ids = catalog (missing {sorted(catalog - set(listed))}, "
                                  f"extra {sorted(set(listed) - catalog)})")
    # System-service entries may cite a local_doc (a man page or plist path; Phase 07
    # ORCHESTRATOR_RULING 裁决二, Phase 06 spec ruling 23:0x); all others cite https.
    local_doc = r"man \d \S+|/\S+\.plist"
    bad = [i for i, (_, src, checked) in listed.items()
           if not (src.startswith("https://") or (i.startswith("system.") and re.fullmatch(local_doc, src)))
           or not re.fullmatch(r"\d{4}-\d{2}-\d{2}", checked)]
    check(not bad, f"every entry has a checked date and an https (system: local_doc) source {bad}")
    local = sorted(i for i, (_, src, _) in listed.items() if not src.startswith("https://"))
    ok(f"local_doc sources: {local}")


COMMANDS = {"ci": ci, "ci-commands": ci_commands, "pages": pages, "history": history, "templates": templates,
            "rendered": rendered, "style": style, "release": release, "readme": readme, "site": site, "render": render, "logo": logo,
            "shots": shots, "insights": insights}

if __name__ == "__main__":
    if len(sys.argv) < 2 or sys.argv[1] not in COMMANDS:
        sys.exit(__doc__)
    COMMANDS[sys.argv[1]](*sys.argv[2:])
    sys.exit(1 if failures else 0)
