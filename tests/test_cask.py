"""Release cask updates must preserve immutable versions and both architectures."""
import importlib.util
from pathlib import Path
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
SPEC = importlib.util.spec_from_file_location('update_cask', ROOT / 'scripts/update-cask.py')
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class CaskTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.cask = Path(self.directory.name) / 'mdiff.rb'
        self.original = (ROOT / 'Casks/mdiff.rb').read_text()
        self.cask.write_text(self.original)
        self.version = '99.0.0'
        self.checksums = '\n'.join(f'{digest * 64}  ./mdiff-{self.version}-{arch}-apple-darwin.zip'
                                   for arch, digest in [('aarch64', 'a'), ('x86_64', 'b')])

    def test_updates_both_architectures_and_is_idempotent(self):
        self.assertTrue(MODULE.update(self.cask, self.version, self.checksums))
        text = self.cask.read_text()
        self.assertIn('version "99.0.0"', text)
        self.assertIn('a' * 64, text)
        self.assertIn('b' * 64, text)
        self.assertFalse(MODULE.update(self.cask, self.version, self.checksums))

    def test_rejects_downgrades_prereleases_and_invalid_versions(self):
        for version in ['0.0.1', '99.0.0-beta.1', '../other', '01.2.3']:
            with self.assertRaises(ValueError):
                MODULE.update(self.cask, version, self.checksums)
            self.assertEqual(self.cask.read_text(), self.original)

    def test_missing_invalid_and_duplicate_checksums_leave_cask_untouched(self):
        for checksums in [self.checksums.splitlines()[0], 'bad checksum', self.checksums + '\n' + self.checksums]:
            with self.assertRaises(ValueError):
                MODULE.update(self.cask, self.version, checksums)
            self.assertEqual(self.cask.read_text(), self.original)

    def test_existing_release_checksums_cannot_be_replaced(self):
        MODULE.update(self.cask, self.version, self.checksums)
        saved = self.cask.read_text()
        with self.assertRaises(ValueError):
            MODULE.update(self.cask, self.version, self.checksums.replace('a' * 64, 'c' * 64))
        self.assertEqual(self.cask.read_text(), saved)


if __name__ == '__main__':
    unittest.main()
