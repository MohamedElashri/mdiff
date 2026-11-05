# mdiff

mdiff is a native code review app for macOS and Linux. Review local changes, GitHub pull requests, and GitLab merge requests with split or unified diffs, threaded feedback, code navigation, and customizable themes and fonts.

[![CI](https://github.com/MohamedElashri/mdiff/actions/workflows/check.yml/badge.svg)](https://github.com/MohamedElashri/mdiff/actions/workflows/check.yml)
[![Release](https://github.com/MohamedElashri/mdiff/actions/workflows/release.yml/badge.svg)](https://github.com/MohamedElashri/mdiff/actions/workflows/release.yml)
[![Version](https://img.shields.io/badge/version-0.1.0--unreleased-blue)](CHANGELOG.md)
[![License: GPL-3.0](https://img.shields.io/badge/license-GPL--3.0-blue)](LICENCE.md)

![mdiff reviewing a feature branch in the native macOS app](assets/demo.png)

## Quick start

Install or update the latest release:

```sh
curl -fsSL https://raw.githubusercontent.com/MohamedElashri/mdiff/main/install.sh | bash
```

Installs to `~/Applications` on macOS or `~/.local/bin` on Linux, with an application menu entry. Supports Apple Silicon, Intel Macs, and Linux x86_64 (Ubuntu 24.04 or compatible, with FUSE 2). Downloads are checksum-verified. **Installation requires the first published release.** [Manual downloads](https://github.com/MohamedElashri/mdiff/releases).

Open mdiff, choose **Open repository**, and select a Git folder. Use **New review** for another comparison or a PR/MR URL. Configure appearance and connections in **Preferences**; press `?` for review shortcuts.

Git is required. Code intelligence needs a language server. Connection tokens stay in memory until you quit. GitLab supports HTTPS hosts and custom ports; installations under URL subpaths are not supported.

## Build

Requires Node.js 26, Rust 1.99, and [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/).

```sh
npm run setup
npm run tauri dev      # development
npm run tauri build    # desktop packages
```

## License

[GPL-3.0-only](LICENCE.md).

## Releases

Push a `v<version>` tag matching the app version to run CI and publish packages, the installer, and checksums using the changelog entry. Prerelease tags are marked as prereleases.

Documentation-only changes skip CI. Code changes run lint and unit tests before cached macOS/Linux builds; releases reuse these stages and build the frontend once.

Action dependencies use Sanad and `actions.lock`. After workflow edits, run `sanad sync --write` and `sanad check`.
