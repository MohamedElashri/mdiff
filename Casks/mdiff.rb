cask "mdiff" do
  arch arm: "aarch64", intel: "x86_64"

  version "0.3.4"
  sha256 arm:   "774b6e96f7f0709ffe7b07c34bc1c6dd469b97bdf19e97c7c536788e20ac699d",
         intel: "0fc8055ce609d05b0aed34fb203c1b8eab7f83aba61769c19cc72626c6ce78ab"

  url "https://github.com/MohamedElashri/mdiff/releases/download/v#{version}/mdiff-#{version}-#{arch}-apple-darwin.zip"
  name "mdiff"
  desc "Native code review for local changes, GitHub, and GitLab"
  homepage "https://github.com/MohamedElashri/mdiff"

  auto_updates true
  depends_on :macos

  app "mdiff.app"

  uninstall quit: "app.mdiff.desktop"
end
