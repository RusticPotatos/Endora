import os
import pathlib
import shutil
import select
import subprocess
import tempfile
import time
import unittest

ROOT=pathlib.Path(__file__).parents[1]

class RecoveryTests(unittest.TestCase):
    def test_existing_boot_upgrade_requires_ssh_recovery_first(self):
        with tempfile.TemporaryDirectory() as folder:
            home=pathlib.Path(folder)
            backup=home/'.local/state/endora-terminal/boot-backup';backup.mkdir(parents=True)
            (backup/'ready').write_text('ready\n')
            commands=home/'commands';commands.mkdir()
            for name, body in {'uname':'echo Linux','id':'echo 1000','systemctl':'exit 1'}.items():
                path=commands/name;path.write_text('#!/bin/sh\n'+body+'\n');path.chmod(0o700)
            result=subprocess.run(['sh',str(ROOT/'boot.sh'),'enable'],env=dict(os.environ,HOME=str(home),PATH=str(commands)+os.pathsep+os.environ['PATH']),capture_output=True,text=True)
            self.assertNotEqual(result.returncode,0)
            self.assertIn('SSH must be active first',result.stderr)

    def test_crashed_appliance_stays_in_recovery_without_shell_commands(self):
        with tempfile.TemporaryDirectory() as folder:
            home=pathlib.Path(folder)
            binary=home/'.local/bin/endora-terminal'
            binary.parent.mkdir(parents=True)
            binary.write_text('#!/bin/sh\nexit 1\n');binary.chmod(0o700)
            commands=home/'commands';commands.mkdir()
            tty=commands/'tty';tty.write_text('#!/bin/sh\nprintf /dev/tty1\n');tty.chmod(0o700)
            child=subprocess.Popen(['sh',str(ROOT/'launcher.sh')],stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,env=dict(os.environ,HOME=str(home),PATH=str(commands)+os.pathsep+os.environ['PATH']))
            try:
                output=bytearray()
                deadline=time.monotonic()+5
                while b'R restart' not in output and time.monotonic()<deadline:
                    if select.select([child.stdout],[],[],.05)[0]:
                        chunk=os.read(child.stdout.fileno(),4096)
                        if not chunk: break
                        output.extend(chunk)
                self.assertIn(b'R restart',output,'A crash must offer recovery, not a shell')
                self.assertIsNone(child.poll(),'A crash must not return to the login shell')
                marker=home/'should-not-exist'
                child.stdin.write(f'touch {marker}\n'.encode());child.stdin.flush();time.sleep(.1)
                self.assertFalse(marker.exists())
                self.assertIsNone(child.poll())
            finally:
                child.terminate();child.communicate(timeout=3)

    def test_browser_keeps_sandbox_and_has_no_shell_menu(self):
        script=(ROOT/'browser-session.sh').read_text()
        self.assertNotIn('--no-sandbox',script)
        self.assertNotIn('xterm',script)
        self.assertIn('key="C-q"',script)
        subprocess.run(['sh','-n',str(ROOT/'browser-session.sh')],check=True)

    def test_locked_portal_requires_filesystem_isolation(self):
        script=(ROOT/'browser-session.sh').read_text()
        self.assertIn('portal-browser.sh',script)
        portal=(ROOT/'portal-browser.sh').read_text()
        self.assertIn('command -v bwrap',portal)
        self.assertIn('--unshare-all --share-net',portal)
        self.assertIn('--tmpfs /home',portal)
        self.assertNotIn('--ro-bind / /',portal)
        self.assertNotIn('--no-sandbox',portal)
