for ((attempt = 0; attempt < 30; attempt++)); do
  if [[ -n "${WAYLAND_DISPLAY:-}" ]]; then
    case "$WAYLAND_DISPLAY" in
      /*) socket="$WAYLAND_DISPLAY" ;;
      *) socket="${XDG_RUNTIME_DIR:-/nonexistent}/$WAYLAND_DISPLAY" ;;
    esac
    if [[ -S "$socket" ]]; then
      exit 0
    fi
  elif [[ -n "${DISPLAY:-}" ]]; then
    # graphical-session.target can precede the window manager on NixOS/i3.
    if xprop -root _NET_SUPPORTING_WM_CHECK 2>/dev/null |
      grep -Eq 'window id # 0x0*[1-9a-fA-F][0-9a-fA-F]*'; then
      exit 0
    fi
  fi
  sleep 1
done

echo "HEX: no ready graphical session. Import its display environment into the systemd user manager; retrying." >&2
exit 1
