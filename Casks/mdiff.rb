cask "mdiff" do
  arch arm: "aarch64", intel: "x86_64"

  version "0.1.3"
  sha256 arm:   "270415e556c9a1b48a4182e5efc560b4cf2e8267ebda3b6335f4020e78a97ecf",
         intel: "cc9f0297da8d86b80d0f90ae92053a2246261a736a61740b5b49d12e4635b3cc"

  url "https://github.com/MohamedElashri/mdiff/releases/download/v#{version}/mdiff-#{version}-#{arch}-apple-darwin.zip"
  name "mdiff"
  desc "Native code review for local changes, GitHub, and GitLab"
  homepage "https://github.com/MohamedElashri/mdiff"

  auto_updates true
  depends_on :macos

  app "mdiff.app"

  uninstall quit: "app.mdiff.desktop"
end
