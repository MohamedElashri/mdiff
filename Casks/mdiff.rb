cask "mdiff" do
  arch arm: "aarch64", intel: "x86_64"

  version "0.3.2"
  sha256 arm:   "904273c8045be9556288a8fb0ba17b420ae0ef71b0f282f339b32dd0df6caecc",
         intel: "ec41802bc9043483439cdd6a26fe33b9d46a8376c553a35b72b226de2fe2b0ff"

  url "https://github.com/MohamedElashri/mdiff/releases/download/v#{version}/mdiff-#{version}-#{arch}-apple-darwin.zip"
  name "mdiff"
  desc "Native code review for local changes, GitHub, and GitLab"
  homepage "https://github.com/MohamedElashri/mdiff"

  auto_updates true
  depends_on :macos

  app "mdiff.app"

  uninstall quit: "app.mdiff.desktop"
end
