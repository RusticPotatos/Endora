import os
import pathlib
import shutil
import subprocess
import tempfile
import unittest

ROOT = pathlib.Path(__file__).parents[1]

class InstallTests(unittest.TestCase):
    def test_install_update_failure_and_rollback_preserve_config(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            home = root / 'home'
            home.mkdir()
            config = home / '.config/endora-terminal/config.toml'
            config.parent.mkdir(parents=True)
            config.write_text('private fixture')
            bundle = root / 'bundle'
            bundle.mkdir()
            for name in ('install.sh', 'boot.sh', 'launcher.sh','browser.sh','browser-session.sh','portal-browser.sh','device-install.sh','desktop.sh','text-browser.sh'):
                shutil.copyfile(ROOT / name, bundle / name)
            shutil.copyfile(ROOT / 'scripts/uninstall.sh', bundle / 'uninstall.sh')
            shutil.copyfile(ROOT / 'scripts/backlight-helper.sh', bundle / 'backlight-helper.sh')
            shutil.copyfile(ROOT / 'scripts/desktop-watch.py', bundle / 'desktop-watch.py')
            executable = bundle / 'endora-terminal'
            executable.write_text('#!/bin/sh\nexit 0\n')
            executable.chmod(0o700)
            env = dict(os.environ, HOME=str(home))
            run = lambda *args: subprocess.run(['sh', str(bundle / 'install.sh'), *args], env=env, capture_output=True)
            (bundle / 'VERSION').write_text('0.1.0')
            self.assertEqual(run().returncode, 0)
            self.assertEqual(run().returncode, 0)
            self.assertTrue((home / '.local/bin/endora').is_symlink())
            self.assertTrue((home / '.local/share/applications/endora-terminal.desktop').is_file())
            (bundle / 'VERSION').write_text('0.2.0')
            self.assertEqual(run().returncode, 0)
            base = home / '.local/share/endora-terminal'
            self.assertEqual(os.readlink(base / 'current'), 'releases/0.2.0')
            self.assertEqual(os.readlink(base / 'previous'), 'releases/0.1.0')
            self.assertEqual(run('rollback').returncode, 0)
            self.assertEqual(os.readlink(base / 'current'), 'releases/0.1.0')
            (bundle / 'VERSION').write_text('0.3.0')
            executable.write_text('#!/bin/sh\nexit 1\n')
            self.assertNotEqual(run().returncode, 0)
            self.assertEqual(os.readlink(base / 'current'), 'releases/0.1.0')
            self.assertEqual(config.read_text(), 'private fixture')
            result = subprocess.run(['sh', str(ROOT / 'scripts/uninstall.sh')], env=env, capture_output=True)
            self.assertEqual(result.returncode, 0)
            self.assertFalse(base.exists())
            self.assertFalse((home / '.local/bin/endora').is_symlink())
            self.assertFalse((home / '.local/share/applications/endora-terminal.desktop').exists())
            self.assertTrue(config.exists())

    def test_symlink_destination_refused(self):
        with tempfile.TemporaryDirectory() as directory:
            home = pathlib.Path(directory) / 'home'
            home.mkdir()
            (home / '.local').symlink_to(pathlib.Path(directory) / 'elsewhere')
            result = subprocess.run(['sh', str(ROOT / 'install.sh')], env=dict(os.environ, HOME=str(home)), capture_output=True)
            self.assertNotEqual(result.returncode, 0)
