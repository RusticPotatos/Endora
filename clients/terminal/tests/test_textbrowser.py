import pathlib
import subprocess
import unittest

ROOT = pathlib.Path(__file__).parents[1]


class TextBrowserScriptTests(unittest.TestCase):
    def setUp(self):
        self.path = ROOT / 'text-browser.sh'
        self.text = self.path.read_text()

    def test_script_is_valid_and_sandboxed(self):
        subprocess.run(['sh', '-n', str(self.path)], check=True)
        self.assertIn('bwrap', self.text)
        self.assertIn('lynx', self.text)
        self.assertIn('--tmpfs /home', self.text, 'the real home must not be visible')
        self.assertIn('--clearenv', self.text)
        self.assertIn('--share-net', self.text)
        for forbidden in ('--bind / ', '--bind "$HOME"', '--ro-bind /home', '/.ssh', 'rm -', 'sudo'):
            self.assertNotIn(forbidden, self.text, forbidden)

    def test_only_http_and_https_destinations_are_accepted(self):
        for bad in ('file:///etc/passwd', 'ftp://example.com', 'javascript:alert(1)', ''):
            result = subprocess.run(['sh', str(self.path), 'portal', bad], capture_output=True)
            self.assertNotEqual(result.returncode, 0, bad)
        result = subprocess.run(['sh', str(self.path), 'nonsense', 'http://example.com'], capture_output=True)
        self.assertNotEqual(result.returncode, 0)


if __name__ == '__main__':
    unittest.main()
