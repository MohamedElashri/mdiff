"""Collect packages while preserving macOS bundle metadata."""
from pathlib import Path
import json
import shutil
import subprocess
import sys

root = Path(__file__).resolve().parents[1]
target = sys.argv[1]
version = json.loads((root / 'package.json').read_text())['version']
bundle = root / 'src-tauri/target' / target / 'release/bundle'
out = root / 'release-artifacts'
out.mkdir(exist_ok=True)
if target.endswith('apple-darwin'):
    app = bundle / 'macos/mdiff.app'
    if not app.is_dir():
        raise SystemExit(f'Missing app bundle: {app}')
    subprocess.run(['ditto', '-c', '-k', '--sequesterRsrc', '--keepParent', str(app),
                    str(out / f'mdiff-{version}-{target}.zip')], check=True)
else:
    for folder, extension in [('deb', 'deb'), ('appimage', 'AppImage')]:
        files = list((bundle / folder).glob(f'*.{extension}'))
        if len(files) != 1:
            raise SystemExit(f'Expected one {extension} package, found {len(files)}')
        shutil.copy2(files[0], out / f'mdiff-{version}-{target}.{extension}')

# Tauri signatures cover the archive bytes, so renaming does not change validity.
if target.endswith('apple-darwin'):
    update = bundle / 'macos/mdiff.app.tar.gz'
    extension = 'app.tar.gz'
else:
    update = next((bundle / 'appimage').glob('*.AppImage'))
    extension = 'AppImage'
for source, suffix in [(update, ''), (Path(str(update) + '.sig'), '.sig')]:
    if not source.is_file() or not source.stat().st_size:
        raise SystemExit(f'Missing signed update artifact: {source}')
    if suffix or target.endswith('apple-darwin'):
        shutil.copy2(source, out / f'mdiff-{version}-{target}.{extension}{suffix}')
