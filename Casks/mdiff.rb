cask "mdiff" do
  arch arm: "aarch64", intel: "x86_64"

  version "0.3.1"
  sha256 arm:   "7185aea916bb64194197fc4885fb098c10545b5f4ec6fe2c3b6a8424412807bc",
         intel: "a5a7ab644ab06da31b19839d1186ebc40bc2804b77986f44d9df81c9ae6c5107"

  url "https://github.com/MohamedElashri/mdiff/releases/download/v#{version}/mdiff-#{version}-#{arch}-apple-darwin.zip"
  name "mdiff"
  desc "Native code review for local changes, GitHub, and GitLab"
  homepage "https://github.com/MohamedElashri/mdiff"

  auto_updates true
  depends_on :macos

  app "mdiff.app"

  uninstall quit: "app.mdiff.desktop"
end
