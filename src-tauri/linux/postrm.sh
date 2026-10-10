#!/bin/sh
# After removal (.deb and .rpm): forget Ivy's udev rule for the virtual keyboard device.
udevadm control --reload-rules 2>/dev/null || true
exit 0
