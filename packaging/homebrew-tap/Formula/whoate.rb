# Template: scripts/release.sh fills @VERSION@ and @CLI_SHA256@.
class Whoate < Formula
  desc "Show which session, pane or app owns the processes eating memory and CPU"
  homepage "https://zhenninglang.github.io/whoate/"
  url "https://github.com/ZhenningLang/whoate/releases/download/v@VERSION@/whoate-v@VERSION@-aarch64-apple-darwin.tar.gz"
  sha256 "@CLI_SHA256@"
  license "MIT"

  depends_on arch: :arm64
  depends_on macos: :sonoma

  def install
    bin.install "whoate"
    pkgshare.install "schema/whoate-v1.schema.json"
    (pkgshare/"skills/whoate").install "skills/whoate/SKILL.md"
  end

  test do
    assert_match "whoate #{version}", shell_output("#{bin}/whoate --version")
    assert_match "claude.mcp", shell_output("#{bin}/whoate insights --list")
  end
end
