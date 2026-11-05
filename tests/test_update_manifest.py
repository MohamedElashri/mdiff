import base64
import importlib.util
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('update_manifest', Path(__file__).resolve().parents[1] / 'scripts/update-manifest.py')
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class ManifestTests(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name)
        self.signature = base64.b64encode(b'fixture signature').decode()
        for target, ext in module.TARGETS.values():
            path = self.root / f'mdiff-0.2.0-{target}.{ext}'
            path.write_bytes(b'fixture package')
            Path(str(path) + '.sig').write_text(self.signature + '\n')

    def tearDown(self):
        self.tmp.cleanup()

    def test_includes_both_macs_and_linux_with_matching_release_urls(self):
        feed = module.manifest('0.2.0', self.root, '- Added feature\n')
        self.assertEqual(set(feed['platforms']), {'darwin-aarch64', 'darwin-x86_64', 'linux-x86_64'})
        self.assertEqual(feed['notes'], '- Added feature\n')
        for value in feed['platforms'].values():
            self.assertTrue(value['url'].startswith('https://github.com/MohamedElashri/mdiff/releases/download/v0.2.0/'))
            self.assertEqual(value['signature'], self.signature)
            self.assertTrue((self.root / value['url'].rsplit('/', 1)[1]).exists())

    def test_rejects_missing_platform_or_signature(self):
        path = next(self.root.glob('*.sig'))
        path.unlink()
        with self.assertRaises(ValueError):
            module.manifest('0.2.0', self.root, '')

    def test_rejects_unsigned_or_invalid_signatures(self):
        for value in ('', 'not a signature!'):
            next(self.root.glob('*.sig')).write_text(value)
            with self.assertRaises(ValueError):
                module.manifest('0.2.0', self.root, '')
