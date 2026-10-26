# Installing whoate

whoate runs on Apple Silicon Macs with macOS 14 (Sonoma) or later. There are two
parts: the `whoate` command-line tool and the `whoate.app` menu bar app. They are
independent; install either or both.

## Homebrew

```sh
brew install zhenninglang/tap/whoate          # CLI
brew install --cask zhenninglang/tap/whoate   # menu bar app
```

The formula installs:

| Path | What |
|---|---|
| `$(brew --prefix)/bin/whoate` | the CLI |
| `$(brew --prefix)/share/whoate/whoate-v1.schema.json` | JSON Schema of `whoate/v1` |
| `$(brew --prefix)/share/whoate/skills/whoate/SKILL.md` | agent instructions (same text as `whoate agent-guide`) |

Upgrade with `brew upgrade whoate` and `brew upgrade --cask whoate`. The app never
checks for updates and never goes online; Homebrew is the update channel.

## The CLI inside the app

`whoate.app` carries its own copy of the CLI in `whoate.app/Contents/Helpers/whoate`.
The cask does not link it into your PATH, so the formula stays the only owner of
`$(brew --prefix)/bin/whoate`:

- Want the command line tool (for yourself or a coding agent)? Install the formula:
  `brew install zhenninglang/tap/whoate`.
- Installed only the app? Open whoate **Settings → Command Line → Install**
  (设置 → 命令行 →「安装」). It links the bundled CLI into the first writable folder
  on your PATH (`/opt/homebrew/bin`, then `~/.local/bin`) and never replaces another
  `whoate` it finds there.
- Installed the app's CLI first and the formula later? Homebrew then reports a link
  conflict; let the formula take over with `brew link --overwrite whoate`.

## GitHub Release

Each [release](https://github.com/ZhenningLang/whoate/releases) has:

- `whoate-v<version>-aarch64-apple-darwin.tar.gz`: the CLI, `LICENSE`, `README.md`,
  `schema/whoate-v1.schema.json` and `skills/whoate/SKILL.md`.
- `whoate-v<version>-macos-arm64-app.zip`: `whoate.app`.
- `SHA256SUMS.txt`: check with `shasum -a 256 -c SHA256SUMS.txt`.

```sh
tar -xzf whoate-v*-aarch64-apple-darwin.tar.gz
sudo cp whoate-v*-aarch64-apple-darwin/whoate /usr/local/bin/
```

A CLI downloaded with a browser also carries the quarantine flag; remove it with
`xattr -d com.apple.quarantine /usr/local/bin/whoate` if macOS refuses to run it.

## First launch of the app (Gatekeeper)

whoate.app is ad-hoc signed and **not notarized**: notarization needs a paid Apple
Developer account, which v1 does without. macOS blocks the first launch:

1. Open whoate.app. macOS reports that it could not verify the app. Click **Done**
   (not "Move to Trash").
2. Open **System Settings → Privacy & Security** (系统设置 → 隐私与安全性). Near the
   bottom, next to the message about whoate, click **Open Anyway** (「仍要打开」).
3. Confirm with your password or Touch ID. Later launches open normally.

Since macOS 15 the old right-click → Open shortcut no longer bypasses this. The
Terminal alternative:

```sh
xattr -d com.apple.quarantine /Applications/whoate.app
```

## Uninstall

```sh
brew uninstall whoate
brew uninstall --cask whoate        # add --zap to also remove caches and preferences
brew untap zhenninglang/tap
```

Uninstalling the cask leaves the link that Settings → Command Line → Install made.
Remove it only if it still points into `whoate.app` (a formula's link is left alone):

```sh
for link in /opt/homebrew/bin/whoate ~/.local/bin/whoate; do
  if [ -L "$link" ] && readlink "$link" | grep -q '/whoate\.app/Contents/Helpers/whoate$'; then
    rm "$link" && echo "removed $link"
  fi
done
```

whoate keeps a cache in `~/Library/Caches/whoate` (override: `WHOATE_CACHE_DIR`).
Your rules file `~/.config/whoate/rules.toml` (override: `WHOATE_RULES`) is yours;
remove it by hand if you no longer want it.

## Build from source

Needs Rust 1.96 and Swift 6 (Xcode or the Command Line Tools).

```sh
git clone https://github.com/ZhenningLang/whoate.git && cd whoate
cargo build -p whoate-cli --release --locked     # target/release/whoate
scripts/bundle-app.sh                            # dist/whoate.app (release, ad-hoc signed)
cargo test --workspace --locked
```

`scripts/release.sh` builds the release assets (both archives, `SHA256SUMS.txt`
and the Homebrew formula and cask) into `dist/release/`; it never uploads anything.
