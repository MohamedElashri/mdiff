"""Update the cask from a stable release's published SHA256SUMS."""
import argparse
from pathlib import Path
import re


def update(cask, version, checksums):
    if not re.fullmatch(r'(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)', version):
        raise ValueError('A stable release version is required')
    original = cask.read_text()
    current = re.search(r'^  version "(\d+\.\d+\.\d+)"$', original, re.M)
    if not current:
        raise ValueError('Cask version is missing')
    if tuple(map(int, version.split('.'))) < tuple(map(int, current[1].split('.'))):
        raise ValueError('Refusing to downgrade the cask')
    entries = {}
    for line in checksums.splitlines():
        match = re.fullmatch(r'([0-9a-f]{64})\s+\*?(?:\./)?([^/]+)', line)
        if not match:
            raise ValueError('Malformed release checksum')
        digest, name = match.groups()
        if name in entries:
            raise ValueError('Duplicate release checksum')
        entries[name] = digest
    hashes = {}
    for arch in ('aarch64', 'x86_64'):
        name = f'mdiff-{version}-{arch}-apple-darwin.zip'
        if name not in entries:
            raise ValueError(f'Missing release checksum: {name}')
        hashes[arch] = entries[name]
    result = re.sub(r'^  version "[^"]+"$', f'  version "{version}"', original, count=1, flags=re.M)
    result, count = re.subn(
        r'^  sha256 arm:   "[0-9a-f]{64}",\n         intel: "[0-9a-f]{64}"$',
        f'  sha256 arm:   "{hashes["aarch64"]}",\n         intel: "{hashes["x86_64"]}"',
        result, count=1, flags=re.M)
    if count != 1:
        raise ValueError('Cask checksum stanza is missing')
    if current[1] == version and result != original:
        raise ValueError('Refusing to replace checksums for an existing version')
    if result != original:
        cask.write_text(result)
    return result != original


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--version', required=True)
    parser.add_argument('--checksums', type=Path, required=True)
    parser.add_argument('--cask', type=Path, default=Path(__file__).resolve().parents[1] / 'Casks/mdiff.rb')
    args = parser.parse_args()
    try:
        changed = update(args.cask, args.version, args.checksums.read_text())
    except ValueError as error:
        parser.error(str(error))
    print('Cask updated' if changed else 'Cask already current')
