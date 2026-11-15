cask "mdiff" do
  arch arm: "aarch64", intel: "x86_64"

  version "0.2.0"
  sha256 arm:   "9fbe6681ceb1b876c94d1a62806ec350a6bc57b7d9d4d14ad0426975b1f8e4cb",
         intel: "07adb7756a66898f227eb344ccf19c4a50151003c716530f54c4eabfff38a5fb"

  url "https://github.com/MohamedElashri/mdiff/releases/download/v#{version}/mdiff-#{version}-#{arch}-apple-darwin.zip"
  name "mdiff"
  desc "Native code review for local changes, GitHub, and GitLab"
  homepage "https://github.com/MohamedElashri/mdiff"

  auto_updates true
  depends_on :macos

  app "mdiff.app"

  uninstall quit: "app.mdiff.desktop"
end
