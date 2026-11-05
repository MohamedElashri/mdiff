#!/usr/bin/env bash
# Install or update mdiff from an official GitHub release.
set -euo pipefail

# EXIT traps run after function-local variables have gone out of scope.
mdiff_tmp=''; mdiff_stage=''; mdiff_backup=''; mdiff_destination=''

main() {
  local blue='' green='' yellow='' red='' reset=''
  if [[ -t 2 && -z "${NO_COLOR:-}" ]]; then
    blue=$'\033[34m'; green=$'\033[32m'; yellow=$'\033[33m'; red=$'\033[31m'; reset=$'\033[0m'
  fi
  info() { printf '%s[info]%s %s\n' "$blue" "$reset" "$*" >&2; }
  warn() { printf '%s[warning]%s %s\n' "$yellow" "$reset" "$*" >&2; }
  die() { printf '%s[error]%s %s\n' "$red" "$reset" "$*" >&2; exit 1; }
  trap 'printf "%s[error]%s Installation failed; see the message above.\n" "${red:-}" "${reset:-}" >&2' ERR
  if [[ "${1:-}" == '--help' ]]; then
    printf 'Usage: bash install.sh [vVERSION]\nInstalls or updates the latest stable release by default. Set NO_COLOR=1 to disable colors.\nSet MDIFF_INSTALL_ROOT to override the default user installation root.\n'
    return
  fi
  [[ $# -le 1 ]] || die 'Expected at most one release tag. Use --help.'
  command -v curl >/dev/null || die 'Install curl and try again.'
  command -v git >/dev/null || warn 'Git is required to review repositories; install it before opening mdiff.'
  local install_root="${MDIFF_INSTALL_ROOT:-$HOME}"
  [[ "$install_root" == /* ]] || die 'MDIFF_INSTALL_ROOT must be an absolute path.'
  local system arch target extension tag="${1:-}" base='https://github.com/MohamedElashri/mdiff/releases'
  system=$(uname -s); arch=$(uname -m)
  case "$system/$arch" in
    Darwin/arm64) target=aarch64-apple-darwin; extension=zip ;;
    Darwin/x86_64) target=x86_64-apple-darwin; extension=zip
      if [[ "$(sysctl -in sysctl.proc_translated 2>/dev/null || true)" == 1 ]]; then target=aarch64-apple-darwin; fi ;;
    Linux/x86_64) target=x86_64-unknown-linux-gnu; extension=AppImage ;;
    *) die "No release package for $system/$arch. Supported: macOS arm64/x86_64 and Linux x86_64." ;;
  esac
  if [[ -z "$tag" ]]; then
    info 'Finding the latest stable release…'
    local url
    url=$(curl --proto '=https' --proto-redir '=https' -fsSL --retry 3 -o /dev/null -w '%{url_effective}' "$base/latest") || die 'No release is available, or GitHub could not be reached.'
    [[ "$url" == "$base/tag/"* ]] || die 'No published release was found.'
    tag=${url##*/}
  fi
  [[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || die "Invalid release tag: $tag"
  local asset="mdiff-${tag#v}-$target.$extension" expected actual
  mdiff_tmp=$(mktemp -d "${TMPDIR:-/tmp}/mdiff-install.XXXXXX")
  # An interrupted macOS replacement restores the previous bundle.
  cleanup() {
    if [[ -n "$mdiff_backup" && -e "$mdiff_backup" && ! -e "$mdiff_destination" ]]; then
      if ! mv "$mdiff_backup" "$mdiff_destination"; then
        printf '[error] Could not restore the previous app. It is preserved at %s\n' "$mdiff_backup" >&2
        return 1
      fi
    fi
    [[ -z "$mdiff_stage" ]] || rm -rf "$mdiff_stage"
    rm -rf "$mdiff_tmp"
  }
  trap cleanup EXIT
  trap 'exit 130' INT
  trap 'exit 143' TERM
  info "Downloading mdiff $tag for $system ($arch)…"
  curl --proto '=https' --proto-redir '=https' -fsSL --retry 3 "$base/download/$tag/$asset" -o "$mdiff_tmp/$asset" || die "Could not download $asset. Check that the release contains this platform."
  curl --proto '=https' --proto-redir '=https' -fsSL --retry 3 "$base/download/$tag/SHA256SUMS" -o "$mdiff_tmp/SHA256SUMS" || die 'Could not download release checksums.'
  expected=$(awk -v file="$asset" '$2 == file || $2 == "./" file {print $1}' "$mdiff_tmp/SHA256SUMS")
  [[ "$expected" =~ ^[0-9a-fA-F]{64}$ ]] || die 'Missing or ambiguous package checksum.'
  if command -v sha256sum >/dev/null; then
    actual=$(sha256sum "$mdiff_tmp/$asset")
  elif command -v shasum >/dev/null; then
    actual=$(shasum -a 256 "$mdiff_tmp/$asset")
  else
    die 'Install sha256sum or shasum to verify the download.'
  fi
  [[ "${actual%% *}" == "$expected" ]] || die 'Checksum mismatch. The existing installation was not changed.'
  info 'Checksum verified.'
  if [[ "$system" == Darwin ]]; then
    mdiff_destination="$install_root/Applications/mdiff.app"
    [[ ! -L "$mdiff_destination" ]] || die "Refusing to replace a symlink: $mdiff_destination"
    if pgrep -x mdiff >/dev/null; then die 'Quit mdiff before updating, then run this installer again.'; fi
    mkdir -p "$install_root/Applications"
    mdiff_stage=$(mktemp -d "$install_root/Applications/.mdiff-install.XXXXXX")
    ditto -x -k "$mdiff_tmp/$asset" "$mdiff_stage"
    [[ -x "$mdiff_stage/mdiff.app/Contents/MacOS/mdiff" ]] || die 'The downloaded app bundle is incomplete.'
    if [[ -e "$mdiff_destination" ]]; then
      info 'Updating the existing application…'
      mdiff_backup="$mdiff_stage/previous.app"
      mv "$mdiff_destination" "$mdiff_backup"
    fi
    mv "$mdiff_stage/mdiff.app" "$mdiff_destination"
    rm -rf "$mdiff_stage"
    mdiff_backup=''
    warn 'This release is not notarized. macOS may require approval in System Settings → Privacy & Security.'
    info 'Open mdiff from your Applications folder.'
  else
    mdiff_destination="$install_root/.local/bin/mdiff"
    [[ ! -L "$mdiff_destination" ]] || die "Refusing to replace a symlink: $mdiff_destination"
    mkdir -p "$install_root/.local/bin" "$install_root/.local/share/applications"
    [[ ! -d "$mdiff_destination" ]] || die "A directory already exists at $mdiff_destination"
    mdiff_stage=$(mktemp "$install_root/.local/bin/.mdiff-install.XXXXXX")
    cp "$mdiff_tmp/$asset" "$mdiff_stage"
    chmod 755 "$mdiff_stage"
    [[ ! -e "$mdiff_destination" ]] || info 'Updating the existing application…'
    mv -f "$mdiff_stage" "$mdiff_destination"
    # Escape characters with special meaning in the desktop Exec string.
    local desktop_exec="$mdiff_destination"
    desktop_exec=${desktop_exec//\\/\\\\}; desktop_exec=${desktop_exec//\"/\\\"}
    desktop_exec=${desktop_exec//\$/\\\$}; desktop_exec=${desktop_exec//\`/\\\`}
    desktop_exec=${desktop_exec//%/%%}
    printf '[Desktop Entry]\nType=Application\nName=mdiff\nComment=Review code changes\nExec="%s"\nIcon=applications-development\nTerminal=false\nCategories=Development;\n' "$desktop_exec" > "$install_root/.local/share/applications/mdiff.desktop"
    if command -v update-desktop-database >/dev/null; then update-desktop-database "$install_root/.local/share/applications" || true; fi
    case ":$PATH:" in *":$install_root/.local/bin:"*) ;; *) warn 'Add ~/.local/bin to PATH to run mdiff from a terminal.' ;; esac
    warn 'Requires Ubuntu 24.04 or a compatible system and FUSE 2 for AppImage support.'
    info 'Open mdiff from your application menu.'
  fi
  printf '%s[success]%s mdiff %s installed at %s\n' "$green" "$reset" "$tag" "$mdiff_destination" >&2
  cleanup
  trap - EXIT INT TERM ERR
}
main "$@"
