#!/bin/sh
# After install or upgrade (.deb and .rpm): set up Ivy's keyboard helper (see /usr/libexec/ivy-keys-setup).
chmod 0755 /usr/libexec/ivy-keys-setup 2>/dev/null || true
/usr/libexec/ivy-keys-setup || true
exit 0
