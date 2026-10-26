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
  render <out dir>         product page in agent-browser, 390x844 and 1440x900, light and dark;
                           network requests stay on file:// (QA12, QA34)
  logo <base> [<end>]      logo sources, icon set, references (QA14)
  shots [<ref>]            site/img/manifest.json matches images, app tree and demo report (QA15)
  insights <whoate>        `insights --list` equals every catalog entry (QA18)
  scope <base> [<end>]     files of this phase's own commits (+ worktree) within the allowlist (QA19)
  readme-top               README first screen and agent usage, with negative controls (QA25)
  demo                     demo GIF limits and frame sources (QA26)
  wording                  interface words match Phases 10 and 11 (QA27)
  support                  support links follow Links.swift, with a negative control (QA28)
  og                       product page Open Graph / Twitter card, demo picture, settings shot (QA29)
  facade [<remote log>]    social preview image and the runbook's repository facade (QA30)
  launch                   launch post drafts (QA31)
  changelog [<release dir> <release log>]  CHANGELOG and release notes (QA32)
  cask-cli [<release dir>] cask without `binary`, CLI caveats and docs (QA33)
  telemetry                no trackers, badges or counters in user-facing files (QA34)

Run from the repository root (verify.sh does). Never writes outside its temp dirs.
"""
import fnmatch, glob, hashlib, html, html.parser, json, os, plistlib, re, subprocess, sys, tempfile, tomllib

INSTALL = "brew install zhenninglang/tap/whoate"
CASK = "brew install --cask zhenninglang/tap/whoate"
PAGES = "https://zhenninglang.github.io/whoate/"
REPO = "https://github.com/ZhenningLang/whoate"
BUNDLE_ID = "io.github.zhenninglang.whoate"
PHASE_COMMIT = re.compile(r"^[a-z]+\(release\):")
TOPICS = {"macos", "memory", "menubar", "activity-monitor", "claude-code", "codex", "tmux", "rust", "swiftui"}
LAUNCH = ["show-hn.md", "v2ex-jike.md", "x-thread.md"]
CAPTURE, SETTINGS = "design_shots.capture", "phase11_checks.py shots"
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
               "header,nav,main,section,figure,figcaption,footer", "--new-inline-tags", "kbd,picture",
               "--new-empty-tags", "source", path)
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
    total = site_bytes()
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
        # QA34: every request the page made, over all four loads, stays on file://.
        requests = re.findall(r"^\[[^\]]+\] [A-Z]+ (\S+)", ab("network", "requests").stdout, re.M)
        outside = sorted({u for u in requests if not u.startswith(("file://", "data:"))})
        check(requests and not outside, f"render {len(requests)} network requests, all file:// {outside}")
        loaded = {os.path.basename(u) for u in requests}
        check({"demo.gif", "settings-cli-en-light.png"} <= loaded, "render loaded the demo GIF and the settings shot")
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
    changed = {f for f in phase_files(base, end) if f.startswith("assets/logo/")}
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


def settings_tabs():
    return re.findall(r'"([a-z]+)"', re.search(r"^TABS = \[(.*)\]$", open("tests/gui/phase11_checks.py").read(), re.M).group(1))


def shots(ref=None):
    manifest = json.load(open("site/img/manifest.json"))
    source = open("tests/gui/design_shots.py").read()
    names = set(re.findall(r'^\s*\("([a-z-]+)", "(?:gallery|panel|main)"', source, re.M))
    check(names, f"design_shots SHOTS {sorted(names)}")
    tabs = settings_tabs()
    check(tabs, f"phase11_checks TABS {tabs}")
    for image in manifest["images"]:
        path = os.path.join("site/img", image["file"])
        check(os.path.isfile(path) and sha256(path) == image["sha256"], f"{path} sha256 matches manifest")
        settings = re.fullmatch(r"settings-([a-z]+)-(zh|en)-(light|dark)", image["shot"])
        if settings:
            check(settings.group(1) in tabs and image.get("generator") == SETTINGS,
                  f"{image['file']} comes from settings tab {settings.group(1)} ({image.get('generator')})")
        else:
            shot = re.sub(r"(-en)?-(light|dark)$", "", image["shot"])
            check(shot in names and image.get("generator") == CAPTURE,
                  f"{image['file']} comes from shot {shot} ({image.get('generator')})")
    tree = run("git", "rev-parse", f"{ref or 'HEAD'}:app").stdout.strip()
    if ref is None:
        dirty = run("git", "status", "--porcelain", "--", "app").stdout.strip()
        check(not dirty, "app/ has no uncommitted changes")
    check(manifest["app_tree"] == tree, f"screenshots taken on app tree {manifest['app_tree'][:12]}, checked tree {tree[:12]}")
    check(manifest["fixture_sha256"] == sha256("tests/gui/design-report.json"), "screenshots replay the current demo report")
    listed = registered(manifest)
    for path in ("README.md", "site/index.html"):
        for img in re.findall(r"img/([\w.-]+\.(?:png|gif))", open(path).read()):
            check(img in listed, f"{path} image {img} is in the manifest")
    present = sorted(os.path.basename(p) for p in glob.glob("site/img/*.png") + glob.glob("site/img/*.gif"))
    check(present == sorted(listed), f"site/img holds only manifest images {sorted(set(present) ^ listed)}")


def registered(manifest):
    """Every image file the manifest accounts for: screenshots, the demo GIF and still, the social card."""
    files = {image["file"] for image in manifest["images"]}
    demo = manifest.get("demo", {})
    files |= {demo.get("file"), demo.get("still", {}).get("file"), manifest.get("social", {}).get("og")}
    return files - {None}


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


# Round 2 (plan.md「第 2 轮」): QA19 range, QA25-QA34. ---------------------------------

ALLOWED = ["README.md", "CHANGELOG.md", ".github/workflows/ci.yml", ".github/workflows/pages.yml", "scripts/release.sh",
           "site/*", "assets/logo/*", "assets/social/*", "packaging/*", "docs/install.md", "docs/releases/*",
           "docs/launch/*", "tests/integration/*"]
FROZEN = ["Cargo.toml", "Cargo.lock", "crates/*", "skills/*", "schema/*", "scripts/bundle-app.sh", "app/*",
          "app/Sources/WhoateKit/Links.swift", "app/Resources/Support/*", ".github/FUNDING.yml", "tests/gui/*",
          "tests/fixtures/*", ".gitignore"]


def range_commits(base, end=None):
    """(sha, subject) of every commit in base..end (HEAD without an end ref)."""
    out = run("git", "log", "--format=%H%x09%s", f"{base}..{end or 'HEAD'}").stdout
    return [tuple(line.split("\t", 1)) for line in out.splitlines() if "\t" in line]


def commit_files(sha):
    return run("git", "diff-tree", "--no-commit-id", "--name-only", "-r", "--root", sha).stdout.splitlines()


def phase_files(base, end=None):
    """This phase's files: commits whose subject starts with `<type>(release):` (Phases 10 and 11
    share the branch), plus uncommitted and untracked files when there is no end ref."""
    files = {f for sha, subject in range_commits(base, end) if PHASE_COMMIT.match(subject) for f in commit_files(sha)}
    if not end:
        files.update(run("git", "diff", "--name-only", "HEAD").stdout.splitlines())
        files.update(run("git", "ls-files", "--others", "--exclude-standard").stdout.splitlines())
    return files


def scope(base, end=None):
    files = sorted(phase_files(base, end))
    print("\n".join(files))
    check(files, f"scope phase 06 changes {len(files)} files")
    if end:
        subject = run("git", "log", "-1", "--format=%s", end).stdout.strip()
        check(PHASE_COMMIT.match(subject), f"scope end commit is this phase's ({subject[:60]!r})")
    outside = [f for f in files if not any(fnmatch.fnmatch(f, a) for a in ALLOWED) or "__pycache__/" in f or f.endswith(".pyc")]
    check(not outside, f"scope every file is in plan.md 可改文件 {outside}")
    touched = [f for f in files if any(fnmatch.fnmatch(f, z) for z in FROZEN)]
    check(not touched, f"scope no frozen file changed {touched}")
    # Negative control: a Phase 10/11 commit on the same branch stays out of the set.
    gui = [(sha, subject) for sha, subject in range_commits(base, end) if subject.startswith("feat(gui):")]
    if check(gui, f"scope negative control: {len(gui)} feat(gui) commits in range"):
        sha, subject = gui[0]
        own = set(commit_files(sha))
        check(not PHASE_COMMIT.match(subject) and own - set(files),
              f"scope negative control: {sha[:7]} ({len(own)} files) is excluded")


def blocks(text):
    """Markdown as (kind, text) blocks: heading, code, list, image, para. Code keeps its body."""
    out, lines, i = [], text.splitlines(), 0
    while i < len(lines):
        line = lines[i]
        if line.startswith("```"):
            j = i + 1
            while j < len(lines) and not lines[j].startswith("```"):
                j += 1
            out.append(("code", line[3:].strip(), "\n".join(lines[i + 1:j])))
            i = j + 1
        elif re.match(r"#{1,6} ", line):
            out.append(("heading", len(line.split()[0]), line.split(" ", 1)[1]))
            i += 1
        elif line.startswith("- "):
            items = []
            while i < len(lines) and (lines[i].startswith("- ") or lines[i].startswith("  ")):
                if lines[i].startswith("- "):
                    items.append(lines[i][2:])
                else:
                    items[-1] += " " + lines[i].strip()
                i += 1
            out.append(("list", items, ""))
        elif line.strip():
            j = i
            while j < len(lines) and lines[j].strip() and not re.match(r"(#{1,6} |```|- )", lines[j]):
                j += 1
            para = lines[i:j]
            kind = "image" if len(para) == 1 and re.fullmatch(r"!\[[^\]]*\]\([^)]+\)", para[0]) else "para"
            out.append((kind, para, ""))
            i = j
        else:
            i += 1
    return out


AGENT_COMMANDS = [r"whoate top --json\b", r"whoate why (\d+|<pid>)( |$)", r"whoate agent-guide\b"]
VERSUS_EN = [r"session|pane|app", r"orphan", r"agent|CLI|JSON"]
VERSUS_ZH = [r"会话|分屏|应用", r"孤儿", r"agent|命令行|JSON"]


def readme_top_problems(text):
    problems = []
    bs = blocks(text)
    h1 = next((n for n, b in enumerate(bs) if b[0] == "heading" and b[1] == 1), None)
    if h1 is None:
        return ["no H1"]
    first_h2 = next((n for n, b in enumerate(bs) if b[0] == "heading" and b[1] == 2), len(bs))
    head = bs[h1 + 1:first_h2]
    want = ["para", "image", "code"]
    if [b[0] for b in head] != want:
        problems.append(f"blocks between H1 and the first H2 are {[b[0] for b in head]}, want {want}")
    else:
        tagline, image, install = head
        sentence = " ".join(tagline[1])
        if len(tagline[1]) > 2 or re.search(r"[一-鿿]", sentence) or not re.search(r"[.—]$", sentence) \
                or re.search(r"[.!?] [A-Z]", sentence):
            problems.append(f"tagline is not one English sentence on <= 2 lines: {sentence!r}")
        alt, target = re.fullmatch(r"!\[([^\]]*)\]\(([^)]+)\)", image[1][0]).groups()
        if target != "site/img/demo.gif" or not alt.strip():
            problems.append(f"demo image {target} alt {alt!r}")
        if install[1] != "sh" or install[2].strip() != INSTALL:
            problems.append(f"install block is {install[2]!r}")
    h2s = [n for n, b in enumerate(bs) if b[0] == "heading" and b[1] == 2]
    if len(h2s) < 2 or "Activity Monitor" not in bs[h2s[0]][2]:
        return problems + ["the first H2 is not about Activity Monitor"]
    items = [b for b in bs[h2s[0] + 1:h2s[1]] if b[0] == "list"]
    items = items[0][1] if len(items) == 1 else []
    if len(items) != 3 or not all(re.search(p, item, re.I) for p, item in zip(VERSUS_EN, items)):
        problems.append(f"Activity Monitor section needs 3 items matching {VERSUS_EN}: {items}")
    if "agent" not in bs[h2s[1]][2].lower():
        problems.append(f"the H2 after Activity Monitor is {bs[h2s[1]][2]!r}, not the agent usage")
    else:
        code = next((b for b in bs[h2s[1] + 1:] if b[0] == "code"), None)
        commands = [l.split("#")[0].strip() for l in (code[2] if code else "").splitlines() if l.strip()][:3]
        if len(commands) < 3 or not all(re.match(p, c) for p, c in zip(AGENT_COMMANDS, commands)):
            problems.append(f"agent section starts with {commands}")
    zh = re.search(r"^#+ .*活动监视器.*\n+((?:- .*\n)+)", text, re.M)
    zh_items = re.findall(r"^- (.*)$", zh.group(1), re.M) if zh else []
    if len(zh_items) != 3 or not all(re.search(p, item) for p, item in zip(VERSUS_ZH, zh_items)):
        problems.append(f"Chinese section lacks the three differences from 活动监视器: {zh_items}")
    if re.search(r"!\[[^\]]*\]\(https?://|<img[^>]+src=\"https?://", text):
        problems.append("README has an external image")
    return problems


def readme_top():
    text = open("README.md").read()
    problems = readme_top_problems(text)
    check(not problems, f"README first screen and agent usage {problems}")
    # Negative control 1: two differences instead of three.
    section = re.search(r"^## How is it different.*?\n(?=## )", text, re.M | re.S).group(0)
    two = section.replace(re.findall(r"^- .*(?:\n  .*)*\n", section, re.M)[-1], "")
    check(readme_top_problems(text.replace(section, two)), "negative control: two differences is caught")
    # Negative control 2: the agent section after "Your own rules".
    agent = re.search(r"^## Use it from a coding agent.*?\n(?=## )", text, re.M | re.S).group(0)
    moved = text.replace(agent, "").replace("## Your own rules", agent + "## Your own rules")
    check(readme_top_problems(moved), "negative control: agent section moved after Your own rules is caught")


def probe(path):
    out = run("ffprobe", "-v", "error", "-select_streams", "v:0", "-show_entries",
              "stream=width,height:format=duration", "-of", "json", path).stdout
    data = json.loads(out)
    return dict(data["streams"][0], duration=float(data["format"]["duration"]))


def site_bytes():
    return sum(os.path.getsize(f) for f in glob.glob("site/**/*", recursive=True) if os.path.isfile(f))


def demo():
    manifest = json.load(open("site/img/manifest.json"))
    gif = "site/img/demo.gif"
    info = probe(gif)
    size = os.path.getsize(gif)
    check(info["duration"] <= 15.0, f"demo lasts {info['duration']}s (<= 15)")
    check(info["width"] <= 960, f"demo is {info['width']}x{info['height']} (width <= 960)")
    check(size <= 2 * 1024 * 1024, f"demo is {size} bytes (<= 2MB)")
    d = manifest.get("demo")
    if not check(d, "manifest has a demo section"):
        return
    check(d["sha256"] == sha256(gif), "demo.gif sha256 matches manifest")
    images = {i["file"]: i for i in manifest["images"]}
    for frame in d["frames"]:
        image = images.get(frame["file"])
        check(image and image["sha256"] == frame["sha256"] and image["shot"] == frame["shot"]
              and image["generator"] in (CAPTURE, SETTINGS),
              f"demo frame {frame['shot']} is a registered demo screenshot ({image and image['generator']})")
    check(len(d["frames"]) >= 3, f"demo has {len(d['frames'])} frames")
    total = sum(f["duration_s"] for f in d["frames"])
    check(abs(total - info["duration"]) <= 0.5, f"demo frame durations {total:.2f}s vs ffprobe {info['duration']}s")
    still = "site/img/" + d["still"]["file"]
    first = d["frames"][0]
    check(os.path.isfile(still) and sha256(still) == d["still"]["sha256"], f"{still} sha256 matches manifest")
    check(d["still"]["source"] == first["file"] and d["still"]["source_sha256"] == first["sha256"],
          f"demo still is the first frame's source {first['file']}")
    check(os.path.isfile(still) and png_size(still) == (info["width"], info["height"]),
          f"demo still is scaled to the GIF size {info['width']}x{info['height']}")
    check(site_bytes() <= 5 * 1024 * 1024, f"site/ is {site_bytes()} bytes (<= 5MB)")


def png_size(path):
    with open(path, "rb") as f:
        head = f.read(26)
    return tuple(int.from_bytes(head[n:n + 4], "big") for n in (16, 20))


def png_colour_type(path):
    with open(path, "rb") as f:
        return f.read(26)[25]


RETIRED = [r"Agent sessions", r"「助手会话」", r"Owners · physical memory", r"归属 · 物理内存", r"\bagent:\S*:pid\b",
           r"session in pane %\d+"]


def wording_files():
    return ["README.md", "site/index.html", *sorted(glob.glob("docs/launch/*.md"))]


def wording():
    for path in wording_files():
        text = html.unescape(open(path).read())
        hits = [w for w in RETIRED if re.search(w, text)]
        check(not hits, f"{path} has no retired interface words {hits}")
    for path in ("README.md", "site/index.html"):
        text = html.unescape(open(path).read())
        check("AI & Agent" in text, f"{path} names the AI & Agent category")
        sentences = [s for s in re.split(r"(?<=[.。])\s+", re.sub(r"<[^>]+>", " ", text))
                     if re.search(r"left column|左栏", s) and re.search(r"number|数字", s)]
        check(sentences and all(re.search(r"footprint|占用", s) for s in sentences),
              f"{path} says the left column's numbers are footprint ({len(sentences)} sentences)")
    manifest = json.load(open("site/img/manifest.json"))
    settings = [i["file"] for i in manifest["images"] if i["shot"].startswith("settings-")]
    page = open("site/index.html").read()
    check(settings and any(f'img/{f}"' in page for f in settings), f"the product page shows a settings shot {settings}")


SUPPORT_WORDS = ["Buy Me a Coffee", "buymeacoffee", "PayPal", "paypal.me", "赞赏", "Sponsor", "Support whoate",
                 "支持作者", "支持 whoate", "爱发电", "afdian", "Ko-fi", "Donate", "打赏", "捐赠"]


def support_items(links_swift):
    body = re.search(r"static let support: \[SupportItem\] = \[(.*?)\]\s*$", links_swift, re.M | re.S)
    body = body.group(1) if body else ""
    return re.findall(r'\.link\("([^"]+)"\)', body), re.findall(r'\.qrCode\(resource:\s*"([^"]+)"\)', body)


def support_docs():
    paths = ["README.md", "site/index.html", "CHANGELOG.md", *glob.glob("docs/launch/*.md"), *glob.glob("docs/releases/*.md")]
    return {p: open(p).read() for p in sorted(paths) if os.path.isfile(p)}


def footer(page):
    match = re.search(r"<footer.*?</footer>", page, re.S)
    return match.group(0) if match else ""


def support_problems(links_swift, docs, funding):
    links, codes = support_items(links_swift)
    problems = []
    if not links and not codes:
        for path, text in docs.items():
            hits = [w for w in SUPPORT_WORDS if w.lower() in text.lower()]
            if hits:
                problems.append(f"{path} mentions {hits} while Links.support is empty")
        if funding:
            problems.append(".github/FUNDING.yml exists while Links.support is empty")
        return problems
    readme_footer = docs["README.md"][docs["README.md"].rfind("\n## "):]
    for url in links:
        if docs["README.md"].count(url) != 1 or url not in readme_footer:
            problems.append(f"README footer must list {url} once")
        if footer(docs["site/index.html"]).count(url) != 1:
            problems.append(f"product page footer must list {url} once")
    for name in codes:
        if not os.path.isfile(os.path.join("app/Resources/Support", name)):
            problems.append(f"QR code app/Resources/Support/{name} is missing")
        for path in ("README.md", "site/index.html"):
            if name not in docs[path]:
                problems.append(f"{path} does not show QR code {name}")
    return problems


def support():
    links_swift = open("app/Sources/WhoateKit/Links.swift").read()
    links, codes = support_items(links_swift)
    docs = support_docs()
    problems = support_problems(links_swift, docs, os.path.exists(".github/FUNDING.yml"))
    check(not problems, f"support links follow Links.swift ({len(links)} links, {len(codes)} QR codes) {problems}")
    # Negative control: the same documents against a Links.swift copy with one made-up link (temp only).
    with tempfile.TemporaryDirectory() as tmp:
        fake = os.path.join(tmp, "Links.swift")
        open(fake, "w").write(links_swift.replace(
            "public static let support: [SupportItem] = []",
            'public static let support: [SupportItem] = [\n        SupportItem(name: "x", target: .link("https://example.invalid/support"), audience: .all),\n    ]'))
        caught = support_problems(open(fake).read(), docs, False)
        check(caught, f"negative control: a support link missing from README and page is caught ({len(caught)})")


def metas(page):
    parser = Page()
    parser.feed(page)
    return {(a.get("property") or a.get("name")): a.get("content") for t, a in parser.attrs if t == "meta"}, parser


def og():
    page = open("site/index.html").read()
    meta, parser = metas(page)
    for key in ("og:title", "og:description", "og:type", "og:image:alt", "twitter:title", "twitter:description"):
        check(meta.get(key), f"site meta {key}")
    check(meta.get("og:url") == PAGES, f"site og:url {meta.get('og:url')}")
    check(meta.get("twitter:card") == "summary_large_image", f"site twitter:card {meta.get('twitter:card')}")
    local = {}
    for key in ("og:image", "twitter:image"):
        url = meta.get(key) or ""
        local[key] = os.path.join("site", url[len(PAGES):]) if url.startswith(PAGES) else None
        check(local[key] and os.path.isfile(local[key]), f"site {key} {url} maps to {local[key]}")
    image = local["og:image"]
    if image and os.path.isfile(image):
        check(png_size(image) == (int(meta.get("og:image:width", 0)), int(meta.get("og:image:height", 0))) == (1280, 640),
              f"og:image is {png_size(image)}, meta says {meta.get('og:image:width')}x{meta.get('og:image:height')}")
    check(os.path.isfile("site/img/og.png") and os.path.isfile("assets/social/preview.png")
          and sha256("site/img/og.png") == sha256("assets/social/preview.png"), "site/img/og.png = assets/social/preview.png")
    # The card embeds screenshots: a reshoot that changes them turns this red until `make_demo.py social` reruns.
    manifest = json.load(open("site/img/manifest.json"))
    social = manifest.get("social", {})
    shots = {i["file"]: i["sha256"] for i in manifest["images"]}
    check(os.path.isfile("site/img/og.png") and social.get("sha256") == sha256("site/img/og.png"), "og.png sha256 matches manifest social")
    stale = [e["file"] for e in social.get("embeds", []) if shots.get(e["file"]) != e["sha256"]]
    check(social.get("embeds") and not stale, f"og.png embeds the current screenshots {stale}")
    picture = re.search(r"<picture>(.*?)</picture>", page, re.S)
    body = picture.group(1) if picture else ""
    check(re.search(r'<source[^>]+srcset="img/demo-still\.png"[^>]+media="\(prefers-reduced-motion: reduce\)"', body)
          and re.search(r'<img[^>]+src="img/demo\.gif"[^>]+alt="[^"]+"', body),
          "site demo GIF in <picture> with a reduced-motion still")
    settings = [a for t, a in parser.attrs if t == "img" and "img/settings-" in a.get("src", "")]
    check(settings and all(a.get("alt", "").strip() for a in settings), f"site settings screenshot with alt {len(settings)}")


def facade(remote_log=None):
    png = "assets/social/preview.png"
    check(png_size(png) == (1280, 640), f"{png} is {png_size(png)}")
    check(os.path.getsize(png) < 1024 * 1024, f"{png} is {os.path.getsize(png)} bytes (< 1MB)")
    check(png_colour_type(png) in (0, 2), f"{png} has no alpha channel (colour type {png_colour_type(png)})")
    runbook = open("packaging/RELEASE_RUNBOOK.md").read()
    section = re.search(r"^## .*仓库门面.*?(?=^## |\Z)", runbook, re.M | re.S)
    if not check(section, "runbook has a 仓库门面 section"):
        return
    section = section.group(0)
    command = re.search(r"gh repo edit (?:[^\n]*\\\n)*[^\n]*", section)
    if not check(command, "runbook 仓库门面 has gh repo edit"):
        return
    command = command.group(0).replace("\\\n", " ")
    topics = set(",".join(re.findall(r"--add-topic ([\w,-]+)", command)).split(","))
    check(topics == TOPICS, f"runbook topics {sorted(topics)} (missing {sorted(TOPICS - topics)}, extra {sorted(topics - TOPICS)})")
    description = re.search(r'--description "([^"]+)"', command)
    check(description and 0 < len(description.group(1)) <= 350, f"runbook description {len(description.group(1)) if description else 0} chars")
    check(re.search(rf"--homepage {re.escape(PAGES)}(\s|$)", command), "runbook homepage is the Pages URL")
    check(re.search(r"Social preview", section) and re.search(r"manual", section, re.I), "runbook social preview is a manual step")
    before = section[:section.index("gh repo edit")]
    check("guard-gitops" in before and "owner" in before, "runbook asks for guard-gitops + owner approval first")
    if remote_log:
        calls = open(remote_log).read()
        check("gh repo edit" not in calls, "no gh repo edit ran during verify")


def section_of(text, title):
    match = re.search(rf"^## {title}\n(.*?)(?=^## |\Z)", text, re.M | re.S)
    return match.group(1) if match else ""


def launch():
    names = sorted(os.listdir("docs/launch")) if os.path.isdir("docs/launch") else []
    if not check(names == LAUNCH, f"docs/launch holds {names}"):
        return
    for name in LAUNCH:
        path = f"docs/launch/{name}"
        text = open(path).read()
        check(re.search(r"草稿|DRAFT", text.split("\n\n")[0]), f"{path} is marked as a draft")
        for title in ("标题", "正文", "配图清单"):
            check(section_of(text, title).strip(), f"{path} has ## {title}")
        for image in re.findall(r"^- `([^`]+)`", section_of(text, "配图清单"), re.M):
            check(os.path.isfile(image) and image.startswith(("site/img/", "assets/social/")), f"{path} picture {image} exists")
        body = section_of(text, "正文")
        check(REPO in body or PAGES in body, f"{path} links the repository or the page")
        for command in re.findall(r"brew install[^\n`]*", text):
            check(command.strip() in (INSTALL, CASK), f"{path} install command {command.strip()!r} matches README")
        check(not re.search(r"\b(stars?|downloads?)\W{0,3}\d|\d[\d,.]*[kK+]?\s*(stars?|downloads?)\b", text, re.I),
              f"{path} promises no star or download numbers")
    title = (section_of(open("docs/launch/show-hn.md").read(), "标题").strip().splitlines() or [""])[0]
    check(title.startswith("Show HN: ") and len(title) <= 80, f"Show HN title {len(title)} chars: {title!r}")
    posts = re.findall(r"^### (\d+)/\n(.*?)(?=^### |^## |\Z)", open("docs/launch/x-thread.md").read(), re.M | re.S)
    check(len(posts) >= 2, f"X thread has {len(posts)} posts")
    for n, post in posts:
        check(len(post.strip()) <= 280, f"X post {n}/ is {len(post.strip())} chars (<= 280)")
    zh = section_of(open("docs/launch/v2ex-jike.md").read(), "正文")
    prose = re.sub(r"```.*?```|`[^`]*`|https?://\S+", "", zh, flags=re.S)
    chars = [c for c in prose if not c.isspace()]
    cjk = [c for c in chars if re.match(r"[一-鿿　-〿＀-￯]", c)]
    ratio = len(cjk) / max(len(chars), 1)
    check(ratio >= 0.5, f"V2EX / 即刻 body is {ratio:.0%} CJK (>= 50%)")


def changelog(release_dir=None, release_log=None):
    version = cargo_version()
    text = open("CHANGELOG.md").read()
    entry = re.search(rf"^## \[{re.escape(version)}\].*?(?=^## \[|\Z)", text, re.M | re.S)
    check(entry and "### Added" in entry.group(0), f"CHANGELOG has [{version}] with ### Added")
    notes_path = f"docs/releases/v{version}.md"
    if not check(os.path.isfile(notes_path), f"release notes {notes_path}"):
        return
    notes = open(notes_path).read()
    check(INSTALL in notes and INSTALL in open("README.md").read(), "release notes install command = README")
    for word in ("Open Anyway", "仍要打开", "xattr -d com.apple.quarantine", "Apple Silicon", "not notarized"):
        check(word in notes, f"release notes mention {word}")
    check(re.search(r"telemetry", notes, re.I), "release notes say there is no telemetry")
    for asset in assets(version):
        check(asset in notes, f"release notes list {asset}")
    if release_dir:
        built = set(os.listdir(release_dir))
        check(set(assets(version)) <= built, f"release notes assets were built {sorted(built)}")
    script = open("scripts/release.sh").read()
    check("--notes-file docs/releases/$tag.md" in script and re.search(r'^tag="v\$version"$', script, re.M),
          "release.sh lists --notes-file docs/releases/$tag.md")
    check("--notes-file docs/releases/v$VERSION.md" in open("packaging/RELEASE_RUNBOOK.md").read(), "runbook gh release create uses --notes-file")
    if release_log:
        check(f"--notes-file docs/releases/v{version}.md" in open(release_log).read(), "release.sh printed the notes file")
    for path in ("CHANGELOG.md", notes_path, "README.md", "site/index.html", *glob.glob("docs/launch/*.md")):
        check("1.0.0" not in open(path).read(), f"{path} does not say 1.0.0")


def cask_cli(release_dir=None):
    paths = ["packaging/homebrew-tap/Casks/whoate.rb"] + ([os.path.join(release_dir, "homebrew-tap/Casks/whoate.rb")] if release_dir else [])
    for path in paths:
        text = open(path).read()
        check(not re.search(r"^\s*binary\b", text, re.M), f"{path} has no binary stanza")
        caveats = re.search(r"caveats <<~EOS\n(.*?)EOS", text, re.S)
        caveats = caveats.group(1) if caveats else ""
        for word in (INSTALL, "brew link --overwrite whoate", "Settings", "Command Line"):
            check(word in caveats, f"{path} caveats mention {word}")
    readme = open("README.md").read()
    install = section_of(readme, "Install")
    check(re.search(r"app also carries the CLI", install) and "brew link --overwrite whoate" in install,
          "README install section: the app carries the CLI, and the link conflict fix")
    doc = open("docs/install.md").read()
    check("Contents/Helpers/whoate" in doc and "brew link --overwrite whoate" in doc, "docs/install.md: the bundled CLI and the conflict fix")
    cleanup = [b for b in fenced(doc, "sh") if re.search(r"\brm\b", b) and "whoate" in b]
    check(cleanup and all(re.search(r"readlink|-L\b", b) for b in cleanup), "docs/install.md removes the link only after checking it")
    check(not re.search(r"^\s*(sudo )?rm (-f )?\S*/bin/whoate\s*$", doc, re.M), "docs/install.md has no unconditional rm of the CLI link")


TRACKERS = r"googletagmanager|google-analytics|plausible|umami|segment\.|hotjar|clarity\.ms|shields\.io|badge|pixel|analytics|gtag|counter"


def telemetry():
    files = ["README.md", *glob.glob("site/**/*.html", recursive=True), *glob.glob("docs/launch/*.md"), *glob.glob("docs/releases/*.md")]
    for path in sorted(files):
        text = open(path).read()
        targets = re.findall(r"https?://[^\s\"'<>)]+", text) + re.findall(r"<script[^>]*>", text) \
            + re.findall(r"<img[^>]+src=\"([^\"]+)\"", text) + re.findall(r"!\[[^\]]*\]\(([^)]+)\)", text)
        hits = [t for t in targets if re.search(TRACKERS, t, re.I)]
        check(not hits, f"{path} has no tracker, badge or counter ({len(targets)} URLs and sources) {hits}")
    page = open("site/index.html").read()
    external = [u for u in re.findall(r'(?:src|srcset|href)="([^"]+)"', page)
                if re.match(r"https?:", u) and not u.startswith(("https://github.com/ZhenningLang/whoate", PAGES))]
    check(not external, f"site references no outside resources or links beyond the repository {external}")


COMMANDS = {"ci": ci, "ci-commands": ci_commands, "pages": pages, "history": history, "templates": templates,
            "rendered": rendered, "style": style, "release": release, "readme": readme, "site": site, "render": render, "logo": logo,
            "shots": shots, "insights": insights, "scope": scope, "readme-top": readme_top, "demo": demo,
            "wording": wording, "support": support, "og": og, "facade": facade, "launch": launch,
            "changelog": changelog, "cask-cli": cask_cli, "telemetry": telemetry}

if __name__ == "__main__":
    if len(sys.argv) < 2 or sys.argv[1] not in COMMANDS:
        sys.exit(__doc__)
    COMMANDS[sys.argv[1]](*sys.argv[2:])
    sys.exit(1 if failures else 0)
