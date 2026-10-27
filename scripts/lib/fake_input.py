#!/usr/bin/env python3
"""Fake input devices through the kernel's uinput, for testing the stylus
daemon (pda-device stylus) without a touchscreen.

  fake_input.py touchscreen NAME SCRIPT   create a resistive-touchscreen-like
                                          device (ABS_X/Y 0..4095, BTN_TOUCH,
                                          direct) and play SCRIPT
  fake_input.py mouse NAME SCRIPT         a relative mouse (REL_X/Y, BTN_LEFT)
  fake_input.py record NAME SECONDS       print the events of the input device
                                          called NAME for SECONDS, one per line:
                                          "<ms> <type> <code> <value>"

SCRIPT is read from a file ("-": stdin), one command per line:
  wait MS              sleep
  down X Y             touch down at screen pixel X,Y (1024x600 screen)
  move X Y             move while touching
  up                   lift
  rel DX DY            mouse motion; click: BTN_LEFT press and release
  click
  remove               destroy the device (unplug)
  create               create it again (replug)
  ready FILE           write FILE once the device exists (for the caller)

Runs as root (or with access to /dev/uinput and /dev/input). Linux, 64-bit.
"""
import fcntl
import os
import struct
import sys
import time

EV_SYN, EV_KEY, EV_REL, EV_ABS = 0, 1, 2, 3
ABS_X, ABS_Y, REL_X, REL_Y = 0, 1, 0, 1
BTN_LEFT, BTN_TOUCH = 0x110, 0x14A
INPUT_PROP_DIRECT = 1
BUS_USB = 3
SCREEN = (1024, 600)
RAW_MAX = 4095


def ioc(direction, nr, size):
    return (direction << 30) | (size << 16) | (ord("U") << 8) | nr


UI_SET_EVBIT = ioc(1, 100, 4)
UI_SET_KEYBIT = ioc(1, 101, 4)
UI_SET_RELBIT = ioc(1, 102, 4)
UI_SET_ABSBIT = ioc(1, 103, 4)
UI_SET_PROPBIT = ioc(1, 110, 4)
UI_DEV_SETUP = ioc(1, 3, 92)
UI_ABS_SETUP = ioc(1, 4, 28)
UI_DEV_CREATE = ioc(0, 1, 0)
UI_DEV_DESTROY = ioc(0, 2, 0)
EVENT = struct.Struct("<qqHHi")


class Device:
    def __init__(self, kind, name):
        self.kind, self.name, self.fd = kind, name, None

    def create(self):
        fd = os.open("/dev/uinput", os.O_WRONLY | os.O_NONBLOCK)
        if self.kind == "touchscreen":
            fcntl.ioctl(fd, UI_SET_EVBIT, EV_KEY)
            fcntl.ioctl(fd, UI_SET_KEYBIT, BTN_TOUCH)
            fcntl.ioctl(fd, UI_SET_EVBIT, EV_ABS)
            fcntl.ioctl(fd, UI_SET_PROPBIT, INPUT_PROP_DIRECT)
            for axis in (ABS_X, ABS_Y):
                fcntl.ioctl(fd, UI_SET_ABSBIT, axis)
                fcntl.ioctl(fd, UI_ABS_SETUP, struct.pack("<H2x6i", axis, 0, 0, RAW_MAX, 0, 0, 0))
            ids = (BUS_USB, 0x0EEF, 0x0001, 1)
        else:
            fcntl.ioctl(fd, UI_SET_EVBIT, EV_KEY)
            fcntl.ioctl(fd, UI_SET_KEYBIT, BTN_LEFT)
            fcntl.ioctl(fd, UI_SET_EVBIT, EV_REL)
            for axis in (REL_X, REL_Y):
                fcntl.ioctl(fd, UI_SET_RELBIT, axis)
            ids = (BUS_USB, 0x046D, 0xC077, 1)
        fcntl.ioctl(fd, UI_DEV_SETUP, struct.pack("<4H80sI", *ids, self.name.encode(), 0))
        fcntl.ioctl(fd, UI_DEV_CREATE)
        self.fd = fd
        # Let udev and readers see the new node.
        time.sleep(0.5)

    def remove(self):
        fcntl.ioctl(self.fd, UI_DEV_DESTROY)
        os.close(self.fd)
        self.fd = None

    def emit(self, *events):
        data = b"".join(EVENT.pack(0, 0, t, c, v) for t, c, v in events)
        os.write(self.fd, data + EVENT.pack(0, 0, EV_SYN, 0, 0))


def raw(x, y):
    return round(x / SCREEN[0] * RAW_MAX), round(y / SCREEN[1] * RAW_MAX)


def play(dev, lines):
    for line in lines:
        words = line.split()
        if not words or words[0].startswith("#"):
            continue
        cmd, args = words[0], words[1:]
        if cmd == "wait":
            time.sleep(int(args[0]) / 1000)
        elif cmd == "down":
            rx, ry = raw(float(args[0]), float(args[1]))
            dev.emit((EV_ABS, ABS_X, rx), (EV_ABS, ABS_Y, ry), (EV_KEY, BTN_TOUCH, 1))
        elif cmd == "move":
            rx, ry = raw(float(args[0]), float(args[1]))
            dev.emit((EV_ABS, ABS_X, rx), (EV_ABS, ABS_Y, ry))
        elif cmd == "up":
            dev.emit((EV_KEY, BTN_TOUCH, 0))
        elif cmd == "rel":
            dev.emit((EV_REL, REL_X, int(args[0])), (EV_REL, REL_Y, int(args[1])))
        elif cmd == "click":
            dev.emit((EV_KEY, BTN_LEFT, 1))
            dev.emit((EV_KEY, BTN_LEFT, 0))
        elif cmd == "remove":
            dev.remove()
        elif cmd == "create":
            dev.create()
        elif cmd == "ready":
            open(args[0], "w").close()
        else:
            raise SystemExit(f"unknown command {cmd}")


def event_node(name):
    for entry in sorted(os.listdir("/sys/class/input")):
        if not entry.startswith("event"):
            continue
        try:
            with open(f"/sys/class/input/{entry}/device/name") as f:
                if f.read().strip() == name:
                    return f"/dev/input/{entry}"
        except OSError:
            pass
    return None


def record(name, seconds):
    deadline = time.monotonic() + seconds
    node = None
    while node is None and time.monotonic() < deadline:
        node = event_node(name)
        time.sleep(0.05)
    if node is None:
        raise SystemExit(f"no input device {name!r}")
    fd = os.open(node, os.O_RDONLY | os.O_NONBLOCK)
    start = time.monotonic()
    print(f"# {node}", flush=True)
    while time.monotonic() < deadline:
        try:
            data = os.read(fd, EVENT.size * 64)
        except BlockingIOError:
            time.sleep(0.01)
            continue
        except OSError:
            break  # device gone
        for i in range(0, len(data) - len(data) % EVENT.size, EVENT.size):
            _, _, t, c, v = EVENT.unpack_from(data, i)
            print(f"{int((time.monotonic() - start) * 1000)} {t} {c} {v}", flush=True)


def main():
    if len(sys.argv) < 4:
        raise SystemExit(__doc__)
    kind, name = sys.argv[1], sys.argv[2]
    if kind == "record":
        record(name, float(sys.argv[3]))
        return
    if kind not in ("touchscreen", "mouse"):
        raise SystemExit(__doc__)
    script = sys.stdin if sys.argv[3] == "-" else open(sys.argv[3])
    dev = Device(kind, name)
    dev.create()
    try:
        play(dev, script.read().splitlines())
    finally:
        if dev.fd is not None:
            dev.remove()


if __name__ == "__main__":
    main()
