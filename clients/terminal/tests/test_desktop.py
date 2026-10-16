import importlib.util
import pathlib
import struct
import subprocess
import unittest

ROOT = pathlib.Path(__file__).parents[1]


def load_watcher():
    spec = importlib.util.spec_from_file_location('desktop_watch', ROOT / 'scripts/desktop-watch.py')
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def key(code, value):
    return struct.pack('llHHi', 0, 0, 1, code, value)


class DesktopWatchTests(unittest.TestCase):
    def test_alt_plus_back_or_escape_asks_to_return_and_other_keys_do_not(self):
        watch = load_watcher()
        state = watch.State()
        # Back/Escape alone never returns, so ordinary desktop use is safe.
        self.assertFalse(watch.pressed(key(1, 1), state))
        self.assertFalse(watch.pressed(key(1, 0), state))
        # Alt held, then Escape or Backspace.
        self.assertFalse(watch.pressed(key(56, 1), state))
        self.assertTrue(watch.pressed(key(1, 1), state))
        self.assertFalse(watch.pressed(key(56, 0), state))
        self.assertFalse(watch.pressed(key(14, 1), state), 'Alt was released')
        self.assertFalse(watch.pressed(key(100, 1), state))
        self.assertTrue(watch.pressed(key(14, 1), state))
        # Key repeat and unrelated events are ignored.
        state = watch.State()
        watch.pressed(key(56, 1), state)
        self.assertFalse(watch.pressed(key(30, 1), state))
        self.assertFalse(watch.pressed(struct.pack('llHHi', 0, 0, 2, 1, 1), state))
        self.assertFalse(watch.pressed(b'short', state))


class DesktopScriptTests(unittest.TestCase):
    def setUp(self):
        self.text = (ROOT / 'desktop.sh').read_text()

    def test_script_is_valid_and_only_manages_the_desktop(self):
        subprocess.run(['sh', '-n', str(ROOT / 'desktop.sh')], check=True)
        self.assertIn('systemctl start lightdm', self.text)
        self.assertIn('systemctl stop lightdm', self.text)
        self.assertIn('chvt', self.text)
        self.assertIn('tty1', self.text)
        for forbidden in ('rm -', 'nmcli', 'ssh', 'iptables', 'passwd', 'disable', 'config.txt'):
            self.assertNotIn(forbidden, self.text, forbidden)

    def test_a_return_icon_is_offered_and_never_overwrites_existing_files(self):
        self.assertIn('Back to Endora', self.text)
        self.assertIn('chvt 1', self.text)
        self.assertIn('test ! -e', self.text)


if __name__ == '__main__':
    unittest.main()
