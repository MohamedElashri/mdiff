# Changelog

<!-- Keep entries concise and user-facing. Use Added, Changed, and Fixed bullets only. -->

## [Unreleased]

## [0.1.3] - 2026-11-06

### Added

- Homebrew installation for macOS.

### Changed

- Require repository trust before enabling code intelligence in each review.

### Fixed

- Bind automatic GitHub environment tokens to their configured host; Enterprise tokens require `GH_HOST`.
- Authenticate private GitHub downloads with the connection token.
- Verify pull request repository hosts and downloaded commits.
- Prevent worktree file reads from following parent-directory symlinks.

## [0.1.2] - 2026-11-05

### Added

- Search added and removed code and saved discussions.
- On-demand symbol references and unified Make commands.

### Changed

- Faster large reviews, language-server updates, and commit comparisons.
- Transactional review storage with preserved migration backups.

### Fixed

- Comment relocation across renames and repeated code.

## [0.1.1] - 2026-11-05

### Changed

- Improved large-review performance.
- Removed upstream AI instructions and skills.

### Fixed

- Clipped toolbar popups and missing icon labels.
- Disappearing diffs after failed comparisons.
- Sensitive-path logging and unsafe paths in upstream tooling.

## [0.1.0] - 2026-11-05

### Added

- About page with project credits and licence information.
- Manually check, install, and restart for signed app updates.
- Open repositories in automatically discovered IDEs.
- Copy review prompts when opening an editor.
- Automatic macOS and Linux installation and updates.
- Native code review for macOS and Linux.
- Working-tree, branch, commit, and range-diff comparisons.
- GitHub pull requests and GitLab merge requests, including self-managed hosts.
- Threaded comments, review drafts, and agent prompt export.
- Code navigation, search, dependency graphs, and image comparisons.
- 40 themes and configurable interface and code fonts.

### Changed

- Adapted the review engine and interface for native desktop use.
- Simplified settings and layouts for smaller windows.

### Fixed

- Preserve and restore the previous macOS app when an update fails.
- Checkbox contrast, configuration feedback, and keyboard handling in settings.
