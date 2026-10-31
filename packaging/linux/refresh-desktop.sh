#!/bin/sh
# The .deb and .rpm postinst and postrm: launchers find Hex's entry and its
# anomaly.hex icon without waiting for the desktop to rescan.
if command -v gtk-update-icon-cache >/dev/null 2>&1; then
  gtk-update-icon-cache -q -t -f /usr/share/icons/hicolor || true
fi
if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database -q /usr/share/applications || true
fi
exit 0
