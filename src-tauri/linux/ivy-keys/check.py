#!/usr/bin/env python3
"""End-to-end check of ivy-keys against the real kernel input layer, with a fake keyboard (python3-evdev).

Run on Linux after building the helper (`cargo build --release` in this folder):
    sudo modprobe uinput evdev    # only where they aren't loaded (WSL)
    sudo python3 check.py target/release/ivy-keys
As root it installs a copy setgid "input" plus 60-ivy-keys.rules (the package's setup), then runs that copy as
SUDO_USER, so it proves what a normal user's Ivy gets. Exits non-zero on the first wrong answer.
"""
import os
import pwd
import select
import shutil
import subprocess
import sys
import time

import evdev
from evdev import UInput, ecodes as e

HERE = os.path.dirname(os.path.abspath(__file__))


def setup(binary):
    """The package's setup, on a private copy: setgid input, and the udev rule for /dev/uinput."""
    test_dir = "/opt/ivy-keys-check"
    os.makedirs(test_dir, exist_ok=True)
    helper = os.path.join(test_dir, "ivy-keys")
    shutil.copy(binary, helper)
    shutil.chown(helper, "root", "input")
    os.chmod(helper, 0o2755)
    shutil.copy(os.path.join(HERE, "..", "60-ivy-keys.rules"), "/etc/udev/rules.d/60-ivy-keys.rules")
    subprocess.run(["udevadm", "control", "--reload-rules"], check=False)
    subprocess.run(["udevadm", "trigger", "--subsystem-match=misc", "--sysname-match=uinput"], check=False)
    subprocess.run(["udevadm", "settle"], check=False)
    return helper


def as_user():
    user = os.environ.get("SUDO_USER")
    if not user:
        return None
    pw = pwd.getpwnam(user)

    def drop():
        os.setgroups([])
        os.setgid(pw.pw_gid)
        os.setuid(pw.pw_uid)

    return drop


class Helper:
    def __init__(self, path):
        self.p = subprocess.Popen([path], stdin=subprocess.PIPE, stdout=subprocess.PIPE, preexec_fn=as_user(), bufsize=0)
        self.buf = b""

    def line(self, timeout=3.0):
        """The next line, read straight from the pipe (a buffered readline could swallow a second one)."""
        deadline = time.time() + timeout
        while b"\n" not in self.buf:
            left = deadline - time.time()
            ready, _, _ = select.select([self.p.stdout], [], [], max(left, 0))
            if not ready:
                return None
            chunk = os.read(self.p.stdout.fileno(), 4096)
            if not chunk:
                return None
            self.buf += chunk
        line, self.buf = self.buf.split(b"\n", 1)
        return line.decode().strip()

    def expect(self, want, timeout=3.0):
        deadline = time.time() + timeout
        seen = []
        while time.time() < deadline:
            got = self.line(deadline - time.time())
            if got is None:
                break
            seen.append(got)
            if got == want:
                return
        sys.exit(f"FAIL: wanted {want!r}, got {seen!r}")

    def send(self, text):
        self.p.stdin.write((text + "\n").encode())
        self.p.stdin.flush()


def press(kb, code, value):
    kb.write(e.EV_KEY, code, value)
    kb.syn()
    time.sleep(0.08)  # several of the helper's 15 ms polls


def quiet(h, what):
    got = h.line(0.3)
    if got is not None:
        sys.exit(f"FAIL: the helper told {what} ({got!r}); other keys may show only inside a Ctrl + Shift press")


def main():
    if os.geteuid() != 0:
        sys.exit("run with sudo (it installs a setgid copy of the helper, like the package does)")
    helper_path = setup(sys.argv[1] if len(sys.argv) > 1 else os.path.join(HERE, "target", "release", "ivy-keys"))
    keys = [e.KEY_LEFTCTRL, e.KEY_LEFTSHIFT, e.KEY_A, e.KEY_T, e.KEY_SPACE]
    with UInput({e.EV_KEY: keys}, name="ivy-keys check keyboard") as kb:
        time.sleep(0.5)  # udev creates the node and sets its group
        h = Helper(helper_path)
        first = h.line(5.0)
        print("helper:", first)
        if not first or not first.startswith("ready "):
            sys.exit(f"FAIL: no ready line ({first!r})")
        _, devices, denied, uinput = first.split()
        if int(devices) < 1 or denied != "0" or uinput != "1":
            sys.exit(f"FAIL: helper can't use the keyboard or the virtual keyboard ({first!r})")
        h.expect("keys 0")
        # Typing tells nothing: keys pressed outside a Ctrl + Shift press give no line at all.
        for code in (e.KEY_A, e.KEY_T, e.KEY_SPACE):
            press(kb, code, 1)
            press(kb, code, 0)
        quiet(h, "plain typing")
        press(kb, e.KEY_LEFTCTRL, 1)
        h.expect("keys 1")
        press(kb, e.KEY_T, 1)
        press(kb, e.KEY_T, 0)
        quiet(h, "the T of Ctrl+T")
        press(kb, e.KEY_LEFTSHIFT, 1)
        h.expect("keys 3")
        press(kb, e.KEY_T, 1)
        h.expect("keys 19")  # Ctrl + Shift + another key
        press(kb, e.KEY_T, 0)
        h.expect("keys 3")
        press(kb, e.KEY_LEFTSHIFT, 0)
        press(kb, e.KEY_LEFTCTRL, 0)
        h.expect("keys 0")
        print("keyboard state: ok")

        # Paste: Shift+Insert from "Ivy virtual keyboard", and not read back as the user's keys.
        ivy = next(
            (evdev.InputDevice(p) for p in evdev.list_devices() if evdev.InputDevice(p).name == "Ivy virtual keyboard"),
            None,
        )
        if ivy is None:
            sys.exit("FAIL: no 'Ivy virtual keyboard' device")
        caps = ivy.capabilities().get(e.EV_KEY, [])
        if sorted(caps) != sorted([e.KEY_LEFTSHIFT, e.KEY_INSERT]):
            sys.exit(f"FAIL: the virtual keyboard has other keys too: {caps}")
        h.send("paste")
        h.expect("pasted 1")
        events = []
        deadline = time.time() + 2
        while len(events) < 4 and time.time() < deadline:
            r, _, _ = select.select([ivy.fd], [], [], 0.2)
            if r:
                events += [(ev.code, ev.value) for ev in ivy.read() if ev.type == e.EV_KEY]
        want = [(e.KEY_LEFTSHIFT, 1), (e.KEY_INSERT, 1), (e.KEY_INSERT, 0), (e.KEY_LEFTSHIFT, 0)]
        if events != want:
            sys.exit(f"FAIL: paste sent {events}, wanted {want}")
        if h.line(0.3) is not None:
            sys.exit("FAIL: the helper read its own Shift+Insert as the user's keys")
        print("paste: ok")

        h.p.stdin.close()
        try:
            h.p.wait(3)
        except subprocess.TimeoutExpired:
            sys.exit("FAIL: helper didn't exit when Ivy closed its input")
        print("exit on close: ok")
    print("PASS")


if __name__ == "__main__":
    main()
