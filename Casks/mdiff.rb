cask "mdiff" do
  arch arm: "aarch64", intel: "x86_64"

  version "0.2.1"
  sha256 arm:   "618932fe9ebea5904720619abcf3bf0fff0ac40b0dd4832f5ae0c930a87a37c8",
         intel: "11c15d88dfa8336f6b03982771f3c81bd8096091333f5752a92a6a583249a7f2"

  url "https://github.com/MohamedElashri/mdiff/releases/download/v#{version}/mdiff-#{version}-#{arch}-apple-darwin.zip"
  name "mdiff"
  desc "Native code review for local changes, GitHub, and GitLab"
  homepage "https://github.com/MohamedElashri/mdiff"

  auto_updates true
  depends_on :macos

  app "mdiff.app"

  uninstall quit: "app.mdiff.desktop"
end
