cask "mdiff" do
  arch arm: "aarch64", intel: "x86_64"

  version "0.3.0"
  sha256 arm:   "7065eb5e218d354700fde33cc00700c607b3d285950b72ac55250694715ca6fc",
         intel: "59cea25deffa176bc4d7c84bf2ec74fcf3ebc6747bbcb10205e848349b37d916"

  url "https://github.com/MohamedElashri/mdiff/releases/download/v#{version}/mdiff-#{version}-#{arch}-apple-darwin.zip"
  name "mdiff"
  desc "Native code review for local changes, GitHub, and GitLab"
  homepage "https://github.com/MohamedElashri/mdiff"

  auto_updates true
  depends_on :macos

  app "mdiff.app"

  uninstall quit: "app.mdiff.desktop"
end
