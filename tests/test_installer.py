"""Exercise the real installer on the host OS with deterministic GitHub release fixtures.

Only network responses, process detection, and injected rename failures are mocked.
Archive extraction, checksums, permissions, installation, and rollback use real tools.
"""
import hashlib
import os
from pathlib import Path
import platform
import shlex
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
BASE = 'https://github.com/MohamedElashri/mdiff/releases'


def mock(tool, args):
    mode = os.environ.get('TEST_MODE', '')
    if tool == 'pgrep':
        return 0 if mode == 'running' else 1
    if tool == 'mv':
        if mode in ('rename-failure', 'restore-failure') and '.mdiff-install.' in args[-2] and (not args[-2].endswith('/previous.app') or mode == 'restore-failure'):
            return 1
        return subprocess.call([os.environ['TEST_REAL_MV'], *args])
    url = next(arg for arg in args if arg.startswith('https://'))
    with open(os.environ['TEST_REQUEST_LOG'], 'a') as log:
        log.write(url + '\n')
    if url == BASE + '/latest':
        if mode == 'no-release':
            return 22
        print(BASE + '/tag/' + os.environ['TEST_LATEST'], end='')
        return 0
    if not url.startswith(BASE + '/download/'):
        return 22
    tag, filename = url[len(BASE + '/download/'):].split('/', 1)
    if mode == 'network-failure' or (mode == 'missing-checksum' and filename == 'SHA256SUMS'):
        return 22
    source = Path(os.environ['TEST_FIXTURES']) / tag / filename
    if not source.is_file():
        return 22
    output = Path(args[args.index('-o') + 1])
    shutil.copyfile(source, output)
    if mode == 'bad-checksum' and filename == 'SHA256SUMS':
        output.write_text('0' * 64 + '  ' + output.read_text().split('  ', 1)[1])
    return 0


class InstallerTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.system = platform.system()
        if cls.system not in ('Darwin', 'Linux'):
            raise unittest.SkipTest('Installer supports macOS and Linux')
        cls.arm = platform.machine() in ('arm64', 'aarch64')
        if cls.system == 'Linux' and cls.arm:
            raise unittest.SkipTest('No Linux ARM release target')
        cls.target = ('aarch64' if cls.arm else 'x86_64') + '-apple-darwin' if cls.system == 'Darwin' else 'x86_64-unknown-linux-gnu'
        cls.extension = 'zip' if cls.system == 'Darwin' else 'AppImage'
        cls.workspace = tempfile.TemporaryDirectory(prefix='mdiff-updater-tests-')
        cls.root = Path(cls.workspace.name)
        cls.fixtures = cls.root / 'releases'
        for tag in ('v0.1.0', 'v0.2.0', 'v0.3.0-beta.1'):
            cls.release(tag)
        cls.bin = cls.root / 'bin'
        cls.bin.mkdir()
        for tool in ('curl', 'pgrep', 'mv'):
            wrapper = cls.bin / tool
            wrapper.write_text('#!/bin/sh\nexec ' + shlex.quote(sys.executable) + ' ' + shlex.quote(str(Path(__file__).resolve())) + f' --mock {tool} "$@"\n')
            wrapper.chmod(0o755)

    @classmethod
    def tearDownClass(cls):
        cls.workspace.cleanup()

    @classmethod
    def release(cls, tag):
        directory = cls.fixtures / tag
        directory.mkdir(parents=True)
        asset = directory / f'mdiff-{tag[1:]}-{cls.target}.{cls.extension}'
        payload = f'#!/bin/sh\nprintf "%s\\n" "{tag}"\n'
        if cls.system == 'Darwin':
            app = directory / 'mdiff.app'
            executable = app / 'Contents/MacOS/mdiff'
            executable.parent.mkdir(parents=True)
            executable.write_text(payload)
            executable.chmod(0o755)
            (app / 'Contents/Info.plist').write_text('<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleExecutable</key><string>mdiff</string></dict></plist>')
            subprocess.run(['ditto', '-c', '-k', '--sequesterRsrc', '--keepParent', str(app), str(asset)], check=True)
        else:
            asset.write_text(payload)
        digest = hashlib.sha256(asset.read_bytes()).hexdigest()
        (directory / 'SHA256SUMS').write_text(f'{digest}  ./{asset.name}\n')

    def setUp(self):
        self.case = tempfile.TemporaryDirectory(dir=self.root)
        self.prefix = Path(self.case.name) / "user with spaces and 'quotes'"
        self.log = Path(self.case.name) / 'requests'
        self.env = dict(os.environ, MDIFF_INSTALL_ROOT=str(self.prefix),
                        PATH=str(self.bin) + os.pathsep + os.environ['PATH'],
                        TEST_FIXTURES=str(self.fixtures), TEST_LATEST='v0.2.0',
                        TEST_REQUEST_LOG=str(self.log), TEST_REAL_MV=shutil.which('mv'),
                        NO_COLOR='1')
        self.binary = self.prefix / ('Applications/mdiff.app/Contents/MacOS/mdiff' if self.system == 'Darwin' else '.local/bin/mdiff')

    def tearDown(self):
        self.case.cleanup()

    def run_installer(self, tag=None, mode='', success=True):
        env = dict(self.env, TEST_MODE=mode)
        result = subprocess.run(['bash', str(ROOT / 'install.sh'), *([tag] if tag else [])],
                                env=env, text=True, capture_output=True)
        self.assertEqual(result.returncode == 0, success, result.stderr)
        self.assertNotIn('unbound variable', result.stderr)
        self.assertNotIn('\x1b', result.stderr)
        return result

    def version(self):
        return subprocess.check_output([str(self.binary)], text=True).strip()

    def old_install(self):
        self.run_installer('v0.1.0')
        self.assertEqual(self.version(), 'v0.1.0')

    def assert_old_preserved(self):
        self.assertEqual(self.version(), 'v0.1.0')
        self.assertFalse(list(self.prefix.rglob('.mdiff-install.*')))

    def test_install_then_latest_upgrade_and_repeat(self):
        self.old_install()
        settings = self.prefix / '.config/mdiff/settings.json'
        settings.parent.mkdir(parents=True)
        settings.write_text('{"theme":"dark"}')
        self.run_installer()
        self.assertEqual(self.version(), 'v0.2.0')
        self.run_installer()
        self.assertEqual(self.version(), 'v0.2.0')
        self.assertEqual(settings.read_text(), '{"theme":"dark"}')
        self.assertIn(BASE + '/latest', self.log.read_text())
        self.assertIn(f'/download/v0.2.0/mdiff-0.2.0-{self.target}.{self.extension}', self.log.read_text())
        if self.system == 'Linux':
            desktop = (self.prefix / '.local/share/applications/mdiff.desktop').read_text()
            self.assertIn(f'Exec="{self.binary}"', desktop)

    def test_explicit_prerelease_does_not_resolve_latest(self):
        self.run_installer('v0.3.0-beta.1')
        self.assertEqual(self.version(), 'v0.3.0-beta.1')
        self.assertNotIn('/latest', self.log.read_text())

    def test_bad_checksum_preserves_old_install(self):
        self.old_install()
        result = self.run_installer(mode='bad-checksum', success=False)
        self.assertIn('Checksum mismatch', result.stderr)
        self.assert_old_preserved()

    def test_missing_checksum_preserves_old_install(self):
        self.old_install()
        self.run_installer(mode='missing-checksum', success=False)
        self.assert_old_preserved()

    def test_failed_download_preserves_old_install(self):
        self.old_install()
        self.run_installer(mode='network-failure', success=False)
        self.assert_old_preserved()

    def test_missing_release_preserves_old_install(self):
        self.old_install()
        result = self.run_installer(mode='no-release', success=False)
        self.assertIn('No release is available', result.stderr)
        self.assert_old_preserved()

    def test_failed_replacement_rolls_back(self):
        self.old_install()
        self.run_installer(mode='rename-failure', success=False)
        self.assert_old_preserved()

    @unittest.skipUnless(platform.system() == 'Darwin', 'macOS backup recovery')
    def test_failed_rollback_keeps_recoverable_backup(self):
        self.old_install()
        result = self.run_installer(mode='restore-failure', success=False)
        self.assertIn('It is preserved at', result.stderr)
        backups = list(self.prefix.rglob('previous.app/Contents/MacOS/mdiff'))
        self.assertEqual(len(backups), 1)
        self.assertEqual(subprocess.check_output([str(backups[0])], text=True).strip(), 'v0.1.0')

    def test_invalid_tag_never_downloads(self):
        self.run_installer('../not-a-release', success=False)
        self.assertFalse(self.log.exists())

    def test_symlink_destination_is_not_overwritten(self):
        dest = self.prefix / ('Applications/mdiff.app' if self.system == 'Darwin' else '.local/bin/mdiff')
        dest.parent.mkdir(parents=True)
        target = Path(self.case.name) / 'keep'
        target.write_text('unchanged')
        dest.symlink_to(target)
        result = self.run_installer(success=False)
        self.assertIn('Refusing to replace a symlink', result.stderr)
        self.assertEqual(target.read_text(), 'unchanged')

    @unittest.skipUnless(platform.system() == 'Darwin', 'macOS running-app protection')
    def test_running_app_preserves_old_install(self):
        self.old_install()
        result = self.run_installer(mode='running', success=False)
        self.assertIn('Quit mdiff', result.stderr)
        self.assert_old_preserved()


if __name__ == '__main__':
    if len(sys.argv) > 1 and sys.argv[1] == '--mock':
        sys.exit(mock(sys.argv[2], sys.argv[3:]))
    unittest.main(verbosity=2)
