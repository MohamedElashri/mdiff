cask "mdiff" do
  arch arm: "aarch64", intel: "x86_64"

  version "0.3.3"
  sha256 arm:   "7bb99d6cdcd5e92bce700829adc1ad3e8728f07c13c670c722a1548d9e1602a7",
         intel: "3f1b3eae816f02b0db7e516a210e951ded69df12135d3c0775d95259e7f7e132"

  url "https://github.com/MohamedElashri/mdiff/releases/download/v#{version}/mdiff-#{version}-#{arch}-apple-darwin.zip"
  name "mdiff"
  desc "Native code review for local changes, GitHub, and GitLab"
  homepage "https://github.com/MohamedElashri/mdiff"

  auto_updates true
  depends_on :macos

  app "mdiff.app"

  uninstall quit: "app.mdiff.desktop"
end
