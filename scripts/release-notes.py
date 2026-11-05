"""Validate desktop versions and extract notes for a release tag."""
import argparse
import json
from pathlib import Path
import re
import tomllib

root = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--tag')
parser.add_argument('--output', type=Path)
args = parser.parse_args()
version = json.loads((root / 'package.json').read_text())['version']
versions = [
    json.loads((root / 'package-lock.json').read_text())['version'],
    json.loads((root / 'src-tauri/tauri.conf.json').read_text())['version'],
    tomllib.loads((root / 'src-tauri/Cargo.toml').read_text())['package']['version'],
]
if any(value != version for value in versions):
    parser.error('Desktop package versions must match.')
if args.tag and args.tag != f'v{version}':
    parser.error(f'Expected tag v{version}, got {args.tag}.')
text = (root / 'CHANGELOG.md').read_text()
match = re.search(rf'^## \[{re.escape(version)}\][^\n]*\n(.*?)(?=^## |\Z)', text, re.M | re.S)
if not match or not match[1].strip():
    parser.error(f'CHANGELOG.md needs entries for {version}.')
notes = match[1].strip() + '\n'
if args.output:
    args.output.write_text(notes)
else:
    print(f'Release metadata ready: v{version}')
