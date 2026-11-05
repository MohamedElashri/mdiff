cask "mdiff" do
  arch arm: "aarch64", intel: "x86_64"

  version "0.1.2"
  sha256 arm:   "f17b807ec0858242bd388eb80df3446a445b3fd5da252fc70f09619f442b14d3",
         intel: "8156d076ae7c5b2b97dd27d4da207f57f249387913da5ce65b90499abcf7c0aa"

  url "https://github.com/MohamedElashri/mdiff/releases/download/v#{version}/mdiff-#{version}-#{arch}-apple-darwin.zip"
  name "mdiff"
  desc "Native code review for local changes, GitHub, and GitLab"
  homepage "https://github.com/MohamedElashri/mdiff"

  auto_updates true
  depends_on :macos

  app "mdiff.app"

  uninstall quit: "app.mdiff.desktop"
end
