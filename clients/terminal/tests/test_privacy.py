import importlib.util
import pathlib
import tempfile
import unittest

spec = importlib.util.spec_from_file_location('privacy', pathlib.Path(__file__).parents[1] / 'scripts/check-private.py')
privacy = importlib.util.module_from_spec(spec)
spec.loader.exec_module(privacy)

class PrivacyTests(unittest.TestCase):
    def test_untracked_content_and_unreadable_files(self):
        with tempfile.TemporaryDirectory() as directory:
            file = pathlib.Path(directory) / 'new.txt'
            # Construct the deliberately non-example address without placing it in Git.
            file.write_text('.'.join(['172', '20', '5', '2']))
            self.assertIn('network address', privacy.inspect(file, []))
            file.write_text('fictional text')
            file.chmod(0)
            self.assertIn('unreadable candidate', privacy.inspect(file, []))

    def test_private_values_and_safe_examples(self):
        with tempfile.TemporaryDirectory() as directory:
            file = pathlib.Path(directory) / 'example.txt'
            file.write_text('192.168.1.10 example@example.com')
            self.assertEqual(privacy.inspect(file, []), [])
            self.assertIn('configured private value', privacy.inspect(file, ['example']))
