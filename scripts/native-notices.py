"""Collect the desktop dependency license files present in the local Cargo/npm cache.
Run after compiling on the target platform; all distribution notices are consolidated in LICENCE.md.
"""
from pathlib import Path
import json, tomllib, os
root = Path(__file__).resolve().parents[1]
engine = tomllib.loads((root/'vendor/diffle/Cargo.lock').read_text())
known = {(p['name'],p['version']) for p in engine['package']}
lock = tomllib.loads((root/'src-tauri/Cargo.lock').read_text())
registries = list((Path(os.environ.get('CARGO_HOME', str(Path.home()/'.cargo')))/'registry/src').glob('*'))
output = ['# mdiff desktop dependency notices\n\nThe engine dependency licences appear above. This supplementary inventory includes desktop build dependencies as well as runtime dependencies.\n']
for package in lock['package']:
    key = package['name'], package['version']
    if key in known or 'source' not in package: continue
    directory = next((r/f'{key[0]}-{key[1]}' for r in registries if (r/f'{key[0]}-{key[1]}').exists()), None)
    if directory is None: continue # Only downloaded/compiled target dependencies are distributed.
    metadata = tomllib.loads((directory/'Cargo.toml').read_text())['package']
    output.append(f'\n## {key[0]} {key[1]}\n\nLicense: {metadata.get("license", "See license file")}\n')
    for name in ['repository','homepage']:
        if metadata.get(name): output.append(f'{name}: {metadata[name]}\n')
    files = [p for p in directory.iterdir() if p.is_file() and p.name.upper().startswith(('LICENSE','LICENCE','COPYING','NOTICE','COPYRIGHT'))]
    if metadata.get('license-file'): files.append(directory/metadata['license-file'])
    for path in sorted(set(files)):
        if path.is_file(): output.append('\n'+path.name+'\n\n'+path.read_text(errors='replace')+'\n')
font_names = [name for name in json.loads((root/'package.json').read_text()).get('dependencies',{}) if name.startswith('@fontsource')]
for name in font_names + ['@tauri-apps/api','@tauri-apps/plugin-updater','@tauri-apps/plugin-dialog','@tauri-apps/plugin-clipboard-manager','@tauri-apps/plugin-opener']:
    directory=root/'node_modules'/name
    metadata=json.loads((directory/'package.json').read_text())
    output.append(f'\n## {name} {metadata["version"]}\n\nLicense: {metadata.get("license", "See package")}\n')
    for path in directory.glob('LICENSE*'):
        if path.is_file(): output.append(path.read_text(errors='replace')+'\n')
licence = root / 'LICENCE.md'
text = licence.read_text()
start, end = '<!-- desktop-licenses:start -->', '<!-- desktop-licenses:end -->'
prefix, remainder = text.split(start, 1)
_, suffix = remainder.split(end, 1)
licence.write_text(prefix + start + '\n' + '\n'.join(output) + '\n' + end + suffix)
