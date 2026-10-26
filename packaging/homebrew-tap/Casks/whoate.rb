# Template: scripts/release.sh fills @VERSION@ and @APP_SHA256@.
cask "whoate" do
  version "@VERSION@"
  sha256 "@APP_SHA256@"

  url "https://github.com/ZhenningLang/whoate/releases/download/v#{version}/whoate-v#{version}-macos-arm64-app.zip"
  name "whoate"
  desc "Menu bar app showing which session, pane or app owns memory and CPU"
  homepage "https://zhenninglang.github.io/whoate/"

  depends_on arch: :arm64
  depends_on macos: :sonoma

  app "whoate.app"

  zap trash: [
    "~/Library/Caches/whoate",
    "~/Library/Preferences/io.github.zhenninglang.whoate.plist",
  ]

  caveats <<~EOS
    whoate.app is ad-hoc signed and not notarized, so macOS blocks the first launch.
    To allow it: System Settings > Privacy & Security > "Open Anyway"
    (系统设置 → 隐私与安全性 →「仍要打开」), then confirm.
    Or remove the quarantine flag:
      xattr -d com.apple.quarantine /Applications/whoate.app

    The app does not put `whoate` on your PATH. For the command line tool:
      brew install zhenninglang/tap/whoate
    or, app only: whoate Settings > Command Line > Install.
    If both are installed and Homebrew reports a link conflict:
      brew link --overwrite whoate
  EOS
end
