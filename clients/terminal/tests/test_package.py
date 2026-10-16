import pathlib
import shutil
import subprocess
import tarfile
import tempfile
import unittest

ROOT = pathlib.Path(__file__).parents[1]

class PackageTests(unittest.TestCase):
    def test_archive_allowlist_and_machine_metadata(self):
        with tempfile.TemporaryDirectory() as directory:
            root = pathlib.Path(directory)
            (root / 'Cargo.toml').write_text('version = "0.1.0"\n')
            for name in ('install.sh', 'boot.sh', 'launcher.sh', 'README.md','browser.sh','browser-session.sh','portal-browser.sh','device-install.sh','desktop.sh','text-browser.sh'):
                shutil.copyfile(ROOT / name, root / name)
            (root / 'scripts').mkdir()
            shutil.copyfile(ROOT / 'scripts/uninstall.sh', root / 'scripts/uninstall.sh')
            shutil.copyfile(ROOT / 'scripts/backlight-helper.sh', root / 'scripts/backlight-helper.sh')
            shutil.copyfile(ROOT / 'scripts/desktop-watch.py', root / 'scripts/desktop-watch.py')
            binary = root / 'target/aarch64-unknown-linux-musl/release/endora-terminal'
            binary.parent.mkdir(parents=True)
            binary.write_text('#!/bin/sh\nexit 0\n')
            binary.chmod(0o700)
            (root / 'secret.local').write_text('fictional confidential fixture')
            subprocess.run(['sh', str(ROOT / 'scripts/package.sh')], cwd=root, check=True, capture_output=True)
            archive = root / 'dist/endora-terminal-0.1.0-aarch64-linux-musl.tar.gz'
            with tarfile.open(archive) as package:
                members = package.getmembers()
                self.assertEqual({m.name for m in members}, {'endora-terminal','install.sh','boot.sh','launcher.sh','uninstall.sh','README.md','VERSION','browser.sh','browser-session.sh','portal-browser.sh','device-install.sh','backlight-helper.sh','desktop.sh','desktop-watch.py','text-browser.sh'})
                for member in members:
                    self.assertEqual((member.uid, member.gid, member.mtime), (0, 0, 0))
                    self.assertEqual((member.uname, member.gname), ('', ''))
                    self.assertEqual(member.pax_headers, {})
            binary.write_text(str(pathlib.Path.home()))
            result = subprocess.run(['sh', str(ROOT / 'scripts/package.sh')], cwd=root, capture_output=True)
            self.assertNotEqual(result.returncode, 0)
