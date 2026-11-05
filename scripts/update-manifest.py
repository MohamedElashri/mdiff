"""Build the Tauri updater feed only after every signed platform artifact exists."""
import argparse
import base64
from datetime import datetime, timezone
import json
from pathlib import Path

TARGETS = {
    'darwin-aarch64': ('aarch64-apple-darwin', 'app.tar.gz'),
    'darwin-x86_64': ('x86_64-apple-darwin', 'app.tar.gz'),
    'linux-x86_64': ('x86_64-unknown-linux-gnu', 'AppImage'),
}


def manifest(version, artifacts, notes):
    platforms = {}
    for platform, (target, extension) in TARGETS.items():
        name = f'mdiff-{version}-{target}.{extension}'
        asset = artifacts / name
        signature = artifacts / (name + '.sig')
        if not asset.is_file() or not asset.stat().st_size:
            raise ValueError(f'Missing updater package: {name}')
        if not signature.is_file():
            raise ValueError(f'Missing updater signature: {signature.name}')
        value = signature.read_text().strip()
        if not value or not base64.b64decode(value, validate=True):
            raise ValueError(f'Invalid updater signature: {signature.name}')
        platforms[platform] = {
            'url': f'https://github.com/MohamedElashri/mdiff/releases/download/v{version}/{name}',
            'signature': value,
        }
    return {'version': version, 'notes': notes,
            'pub_date': datetime.now(timezone.utc).isoformat().replace('+00:00', 'Z'),
            'platforms': platforms}


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--artifacts', type=Path, default=Path('release-artifacts'))
    parser.add_argument('--notes', type=Path, default=Path('release-notes.txt'))
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    version = json.loads((root / 'package.json').read_text())['version']
    result = manifest(version, args.artifacts, args.notes.read_text())
    (args.artifacts / 'latest.json').write_text(json.dumps(result, indent=2) + '\n')
