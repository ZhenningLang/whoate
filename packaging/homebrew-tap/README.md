# ZhenningLang/homebrew-tap

Homebrew formulae and casks for [whoate](https://github.com/ZhenningLang/whoate).

```sh
brew install zhenninglang/tap/whoate          # CLI
brew install --cask zhenninglang/tap/whoate   # menu bar app
```

Apple Silicon, macOS 14 or later. The app is not notarized: on first launch open
System Settings > Privacy & Security and click "Open Anyway", or run
`xattr -d com.apple.quarantine /Applications/whoate.app`.

The files here are rendered by `scripts/release.sh` in the whoate repository;
edit the templates in `packaging/homebrew-tap/` there, not these copies.
