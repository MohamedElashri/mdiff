# Changelog

<!-- Keep entries concise and user-facing. Use Added, Changed, and Fixed bullets only. -->

## [Unreleased]

## [0.3.2] - 2026-11-23

### Changed

- Refresh About with project links and an optional Ko-fi tip link.

## [0.3.1] - 2026-11-22

### Fixed

- Agent detection, activity, tool checks, and error feedback.
- Preference saving, review navigation, image loading, and monitoring feedback.

### Changed

- Unified Preferences and clearer repository tools and naming.

## [0.3.0] - 2026-11-22

### Added

- Review tabs, workspaces, commit history, and configurable shortcuts.
- Git operations, conflict recovery, worktrees, and terminals.
- Hosting reviews/checks, opt-in agent activity, and an attention inbox.

### Changed

- Customizable appearance, a French preview, and stronger review isolation.

## [0.2.1] - 2026-11-19

### Changed

- Faster startup and less background loading in large reviews.
- Faster worktree refreshes, searches, and viewed-mark updates.
- Smaller downloads for pull requests from other repositories.

## [0.2.0] - 2026-11-15

### Added

- Optional GitHub PR creation and comment transfer.

### Fixed

- Keep reviews and unsent comments after failed opens.
- Fix stale GitLab discussions, editor actions, and duplicate toolbar controls.
- Keep GitLab credentials out of Git traces.

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
