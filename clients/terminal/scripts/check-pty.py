#!/usr/bin/env python3
"""Exercise resize, navigation, exit and terminal restoration in a real PTY."""
import os
import fcntl
import pathlib
import pty
import select
import signal
import subprocess
import struct
import sys
import tempfile
import termios
import time

binary = str(pathlib.Path(sys.argv[1]).resolve())
terminal_type = sys.argv[2] if len(sys.argv) > 2 else 'xterm-256color'
with tempfile.TemporaryDirectory() as directory:
    master, slave = pty.openpty()
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 32, 80, 0, 0))
    def child_terminal():
        os.setsid()
        fcntl.ioctl(slave, termios.TIOCSCTTY, 0)
    original = termios.tcgetattr(slave)
    process = subprocess.Popen(['sh', '-c', '"$1"; rc=$?; printf "\\n__ENDORA_DONE_%s__\\n" "$rc"; exec sleep 60', 'sh', binary], stdin=slave, stdout=slave, stderr=slave,
                               preexec_fn=child_terminal,
                               env=dict(os.environ, HOME=directory, TERM=terminal_type))
    output = bytearray()
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        if select.select([master], [], [], 0.1)[0]:
            output.extend(os.read(master, 65536))
        if b'Endora' in output:
            break
    if b'Endora' not in output:
        process.kill()
        process.wait()
        raise AssertionError('Interface did not render')
    def drain_for(seconds):
        until = time.monotonic() + seconds
        while time.monotonic() < until:
            if select.select([master], [], [], 0.05)[0]:
                output.extend(os.read(master, 65536))
    # Finish consuming the first frame before delivering resize and input.
    drain_for(0.3)
    # Mouse capture is enabled, and SGR wheel events do not enter chat text.
    if terminal_type == 'linux':
        assert b'\x1b[?1000h' not in output, 'Unsupported mouse codes sent to console'
    else:
        assert b'\x1b[?1000h' in output
        os.write(master, b'\x1b[<64;4;4M\x1b[<65;4;4M')
    drain_for(0.3)
    # The centered passcode box owns the keyboard until the first unlock: typed letters
    # are hidden, a wrong passcode is refused, and Esc opens Settings > Wi-Fi.
    assert b'Passcode' in output, 'Passcode box missing'
    os.write(master, b'secret')
    drain_for(0.3)
    assert b'secret' not in output, 'Passcode must never be echoed'
    os.write(master, b'\r')
    drain_for(0.3)
    os.write(master, b'\x1b')
    drain_for(0.3)
    fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack('HHHH', 24, 60, 0, 0))
    os.killpg(process.pid, signal.SIGWINCH)
    drain_for(0.3)
    # Locked Settings cannot open a shell. Ctrl-Q terminates the app; the appliance
    # launcher owns fail-closed recovery outside the application.
    os.write(master, b'\x11')
    deadline = time.monotonic() + 5
    while b'__ENDORA_DONE_0__' not in output and time.monotonic() < deadline:
        if select.select([master], [], [], 0.1)[0]:
            output.extend(os.read(master, 65536))
    if b'__ENDORA_DONE_0__' not in output:
        process.kill()
        process.wait()
        raise AssertionError('Interface did not exit')
    assert termios.tcgetattr(slave) == original, 'Terminal state was not restored'
    os.killpg(process.pid, signal.SIGTERM)
    process.wait(timeout=5)
    os.close(master)
    os.close(slave)
    print('PTY rendering and terminal restoration passed')
