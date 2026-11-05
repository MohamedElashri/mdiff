# mdiff

Native code review for macOS and Linux. Open a repository, review changes, and leave feedback—all in one desktop window.

[![CI](https://github.com/MohamedElashri/mdiff/actions/workflows/check.yml/badge.svg)](https://github.com/MohamedElashri/mdiff/actions/workflows/check.yml)
[![Release](https://github.com/MohamedElashri/mdiff/actions/workflows/release.yml/badge.svg)](https://github.com/MohamedElashri/mdiff/actions/workflows/release.yml)
[![Version](https://img.shields.io/badge/version-0.1.0--unreleased-blue)](CHANGELOG.md)
[![License: GPL-3.0](https://img.shields.io/badge/license-GPL--3.0-blue)](LICENSE)

![mdiff reviewing a feature branch in the native macOS app](assets/demo.png)

## Features

- Split and unified diffs for working changes, branches, commits, and range-diffs.
- GitHub pull requests and GitLab merge requests, including self-managed hosts.
- Threaded comments, review drafts, and agent prompt export.
- Code navigation, search, dependency graphs, and image comparisons.
- 40 themes, independent app/code fonts, and compact layouts.

## Get started

**v0.1.0 is in preparation.** Packages will be available from [Releases](https://github.com/MohamedElashri/mdiff/releases).

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

[GPL-3.0-only](LICENSE). Built on [Diffle](https://github.com/moritzwilksch/diffle), with its Apache-2.0 license and [attribution](vendor/diffle/MDIFF-PROVENANCE.md) retained. Dependencies retain their respective licenses.
