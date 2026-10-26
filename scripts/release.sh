#!/usr/bin/env bash
# Build the release assets locally: Apple Silicon CLI tarball, whoate.app zip
# (ad-hoc signed, not notarized, U-DEC-1), SHA256SUMS.txt and the rendered
# Homebrew formula and cask. Nothing leaves this machine: no push, no tag, no
# upload. The remote steps are printed at the end (packaging/RELEASE_RUNBOOK.md).
#   scripts/release.sh [--out <dir>] [--allow-dirty]
# --allow-dirty is for verification only; the build info then records it.
set -euo pipefail
exec </dev/null
root=$(cd "$(dirname "$0")/.." && pwd)
# Build outputs must land in $root/target: the packing below and app/Package.swift read them there.
unset CARGO_TARGET_DIR

out=""
allow_dirty=0
while (($#)); do
    case $1 in
    --out) out=${2:?--out needs a directory}; shift 2 ;;
    --allow-dirty) allow_dirty=1; shift ;;
    -h | --help) sed -n '2,7p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
done

[[ $(uname -m) == arm64 ]] || { echo "release builds need an Apple Silicon Mac" >&2; exit 1; }
dirty=$(git -C "$root" status --porcelain)
if [[ -n $dirty && $allow_dirty == 0 ]]; then
    echo "working tree is not clean; commit first (or --allow-dirty for a test build)" >&2
    exit 1
fi
version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)
[[ -n $version ]] || { echo "no version in Cargo.toml" >&2; exit 1; }
commit=$(git -C "$root" rev-parse HEAD)
tag="v$version"
# Asset names: the formula and cask templates build the same names from the version.
cli_asset="whoate-$tag-aarch64-apple-darwin.tar.gz"
app_asset="whoate-$tag-macos-arm64-app.zip"
if [[ -z $out ]]; then
    out="$root/dist/release/$tag"
    [[ -n $dirty ]] && out="$out-dirty-${commit:0:7}"
fi
mkdir -p "$out"
out=$(cd "$out" && pwd)
rm -f "$out/$cli_asset" "$out/$app_asset" "$out/SHA256SUMS.txt" "$out/BUILD_INFO.txt"
rm -rf "$out/homebrew-tap"

# CLI: rusqlite's bundled SQLite would otherwise target the build host's macOS.
export MACOSX_DEPLOYMENT_TARGET=14.0
cargo build -p whoate-cli --release --locked --manifest-path "$root/Cargo.toml"
cli="$root/target/release/whoate"
bundle_log=$("$root/scripts/bundle-app.sh")
grep -q '^BUNDLE_OK ' <<<"$bundle_log" || { echo "$bundle_log" >&2; echo "bundle-app.sh did not finish" >&2; exit 1; }

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
stage="$work/whoate-$tag-aarch64-apple-darwin"
mkdir -p "$stage/schema" "$stage/skills/whoate"
cp "$cli" "$stage/whoate"
cp "$root/LICENSE" "$root/README.md" "$stage/"
cp "$root/schema/whoate-v1.schema.json" "$stage/schema/"
cp "$root/skills/whoate/SKILL.md" "$stage/skills/whoate/"
COPYFILE_DISABLE=1 tar -C "$work" --uid 0 --gid 0 --uname root --gname root \
    -czf "$out/$cli_asset" "$(basename "$stage")"
ditto -c -k --sequesterRsrc --keepParent "$root/dist/whoate.app" "$out/$app_asset"
(cd "$out" && shasum -a 256 "$cli_asset" "$app_asset" >SHA256SUMS.txt)

sha_of() { awk -v f="$1" '$2 == f { print $1 }' "$out/SHA256SUMS.txt"; }
for kind in Formula Casks; do
    mkdir -p "$out/homebrew-tap/$kind"
    sed -e '/^# Template:/d' -e "s/@VERSION@/$version/g" -e "s/@CLI_SHA256@/$(sha_of "$cli_asset")/g" \
        -e "s/@APP_SHA256@/$(sha_of "$app_asset")/g" \
        "$root/packaging/homebrew-tap/$kind/whoate.rb" >"$out/homebrew-tap/$kind/whoate.rb"
done
cp "$root/packaging/homebrew-tap/README.md" "$out/homebrew-tap/README.md"
if grep -q '@[A-Z0-9_]*@' "$out/homebrew-tap/Formula/whoate.rb" "$out/homebrew-tap/Casks/whoate.rb"; then
    echo "unrendered placeholder in the formula or cask" >&2
    exit 1
fi

# Self-checks on what was packed, not on the build tree.
check="$work/check"
mkdir -p "$check"
tar -C "$check" -xzf "$out/$cli_asset"
bin="$check/$(basename "$stage")/whoate"
[[ $(lipo -archs "$bin") == arm64 ]] || { echo "CLI is not arm64-only" >&2; exit 1; }
minos=$(vtool -show-build "$bin" | awk '$1 == "minos" { print $2; exit }')
[[ $minos == 14.0 ]] || { echo "CLI minos is $minos, want 14.0" >&2; exit 1; }
[[ $("$bin" --version) == "whoate $version" ]] || { echo "CLI version mismatch" >&2; exit 1; }
ditto -x -k "$out/$app_asset" "$check"
app="$check/whoate.app"
codesign --verify --strict "$app"
# Read nm once: under pipefail, `nm | grep -q` can SIGPIPE nm.
symbols=$(nm "$app/Contents/MacOS/Whoate")
if grep -q '_whoate_engine_new_from_fixture' <<<"$symbols"; then
    echo "release app exports the debug fixture entry point" >&2
    exit 1
fi
[[ $(plutil -extract CFBundleShortVersionString raw "$app/Contents/Info.plist") == "$version" ]] ||
    { echo "app version differs from Cargo.toml" >&2; exit 1; }
(cd "$out" && shasum -a 256 -c SHA256SUMS.txt >/dev/null)

{
    echo "version $version"
    echo "tag $tag"
    echo "commit $commit"
    echo "dirty $([[ -n $dirty ]] && echo yes || echo no)"
    echo "built_at $(date -u +%Y-%m-%dT%H:%M:%SZ)"
} >"$out/BUILD_INFO.txt"

cat <<EOF
RELEASE_OK $out
  $cli_asset
  $app_asset
  SHA256SUMS.txt
  homebrew-tap/Formula/whoate.rb, homebrew-tap/Casks/whoate.rb, homebrew-tap/README.md
  BUILD_INFO.txt (commit ${commit:0:7}, dirty=$([[ -n $dirty ]] && echo yes || echo no))

Remote steps (not run; owner approval and guard-gitops first, see packaging/RELEASE_RUNBOOK.md):
  git push origin main            # fast-forward or merge only, never squash/rebase
  # only when HEAD == origin/main ($commit): the tag must match the build
  gh release create $tag --target $commit "$out/$cli_asset" "$out/$app_asset" "$out/SHA256SUMS.txt" --title "whoate $tag" --notes-file docs/releases/$tag.md
  gh repo create ZhenningLang/homebrew-tap --public   # first release only
  # copy $out/homebrew-tap/* into the tap repository, commit, push
  gh api -X POST repos/ZhenningLang/whoate/pages -f build_type=workflow   # first release only
EOF
