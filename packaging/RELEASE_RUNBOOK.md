# Release runbook

Everything here is a remote action. Run it only after `guard-gitops` and an
explicit owner approval in the current conversation; record each command's
output. `scripts/release.sh` builds the assets locally and prints the same list.

```sh
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)   # 0.1.0
OUT=dist/release/v$VERSION
```

## 1. Push `main`

Fast-forward or a merge commit only. **Never squash or rebase**: the frozen
contract tests run `git show <sha>:<path>` against `7f9c201`, `7f63627`,
`8eef7e0` and `e0d5440`, and CI fails for good if those commits are not in
`main`'s history.

```sh
git checkout main && git merge --ff-only lr/whoate-v1-per-software-process-attribution
for sha in 7f9c201 7f63627 8eef7e0 e0d5440; do git merge-base --is-ancestor $sha main || echo "MISSING $sha"; done
git push origin main
gh run list --workflow CI --branch main --limit 1     # record the run URL
gh run watch <run-id> --exit-status                   # cargo test, clippy, swift build all green
```

The first run is the first time the runner's Swift compiles `app/`; if
`swift build` fails there, stop and open a fix round.

The first push also starts the Pages workflow (it changes `site/**`). Pages is
not enabled until step 5, so that run is expected to fail; step 5 reruns it.

## 2. Build the assets (clean tree)

```sh
git status --porcelain                # must be empty
scripts/release.sh                    # ends with RELEASE_OK $OUT
cat $OUT/SHA256SUMS.txt $OUT/BUILD_INFO.txt   # dirty no, commit = the pushed main
```

## 3. GitHub Release

The tag must point at the commit the assets were built from. The subshell
stops the step without closing your terminal when HEAD is not origin/main:

```sh
git fetch origin main
(
  [ "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" ] || { echo "STOP: HEAD is not origin/main"; exit 1; }
  gh release create v$VERSION --target "$(git rev-parse HEAD)" \
    $OUT/whoate-v$VERSION-aarch64-apple-darwin.tar.gz \
    $OUT/whoate-v$VERSION-macos-arm64-app.zip \
    $OUT/SHA256SUMS.txt \
    --title "whoate v$VERSION" --notes "Apple Silicon, macOS 14+. The app is not notarized; see the README for the first-launch steps."
)
gh release view v$VERSION --json assets --jq '.assets[].name'
```

## 4. Homebrew tap (`ZhenningLang/homebrew-tap`)

First release only: `gh repo create ZhenningLang/homebrew-tap --public --description "Homebrew tap for whoate"`.

```sh
TAP=$(mktemp -d)
gh repo clone ZhenningLang/homebrew-tap "$TAP"
cp -R $OUT/homebrew-tap/. "$TAP/"
git -C "$TAP" add Formula/whoate.rb Casks/whoate.rb README.md
git -C "$TAP" commit -m "whoate $VERSION"
git -C "$TAP" push origin HEAD
```

## 5. GitHub Pages

First release only:

```sh
gh api -X POST repos/ZhenningLang/whoate/pages -f build_type=workflow
gh workflow run Pages --ref main
gh run list --workflow Pages --limit 1
curl -sI https://zhenninglang.github.io/whoate/ | head -1      # HTTP/2 200
```

## 6. Install check (U-ACC-7)

Homebrew writes the global prefix, so check and clean up around it. The
temporary HOME lives only inside the subshell, so step 7 runs with the real one.

```sh
brew list whoate 2>&1          # must say not installed; otherwise stop and report
(
  export HOME=$(mktemp -d)     # a user directory that never had whoate
  brew install zhenninglang/tap/whoate
  whoate top; echo "exit $?"   # 0, or 1 for an "act" diagnosis
  whoate --version
  ls "$(brew --prefix)/share/whoate"
  brew uninstall whoate && brew untap zhenninglang/tap
)
brew list whoate 2>&1          # not installed again
```

## 7. Cask and Gatekeeper (manual, qa.md QA23/QA24)

```sh
brew list --cask whoate 2>&1   # must say not installed
brew install --cask zhenninglang/tap/whoate
open /Applications/whoate.app  # blocked the first time; System Settings > Privacy & Security > Open Anyway
brew uninstall --cask whoate
```

If Homebrew refuses the unnotarized cask, record the full output, point README
and the tap README at the Release zip only, and bring notarization (U-DEC-1)
back to the owner.
