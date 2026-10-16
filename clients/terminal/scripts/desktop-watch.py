#!/usr/bin/env python3
"""While the desktop is up, Alt + Back (Esc) or Alt + Backspace on the Q20 returns to Endora.

The Q20 has no function keys, so the usual Ctrl+Alt+F1 is unavailable. This reads the
keyboard's raw events (no text is recorded) and runs `chvt 1` when the chord is pressed.
"""
import glob
import os
import select
import struct
import subprocess
import sys

EVENT = struct.Struct('llHHi')
KEY = 1
ALT = {56, 100}
BACK = {1, 14}


class State:
    def __init__(self):
        self.alt = False


def pressed(record, state):
    """True when this record completes the chord. Updates `state` as Alt goes down and up."""
    if len(record) != EVENT.size:
        return False
    _, _, kind, code, value = EVENT.unpack(record)
    if kind != KEY:
        return False
    if code in ALT:
        state.alt = value != 0
        return False
    return state.alt and value == 1 and code in BACK


def keyboards():
    return [
        p for p in glob.glob('/dev/input/by-id/*')
        if 'HACKBerryPiQ20' in p and p.endswith('event-kbd')
    ]


def main():
    files = [open(p, 'rb', buffering=0) for p in keyboards()]
    if not files:
        sys.exit('No Q20 keyboard found')
    state = State()
    while True:
        ready, _, _ = select.select(files, [], [], 1.0)
        for f in ready:
            record = f.read(EVENT.size)
            if record and pressed(record, state):
                subprocess.run(['chvt', '1'])
                return


if __name__ == '__main__':
    if os.geteuid() != 0:
        sys.exit('Run through sudo: it reads keyboard events')
    main()
