# W33 brand mark: the boot splash (two moments), the desktop (panel mark and taskbar), the Apps
# overlay, the system menu, Ajustes > Sobre and the browser's kitsune://sobre page.
# Run in UEFI (1280x800); dark by default, light with QEMU_EXTRA="-rtc base=2026-10-08T12:00:00".
source "$(dirname "$0")/../lib.sh"
until grep -aq "ui text engine ready" "$OUT/serial.log" 2>/dev/null; do sleep 0.2; done
sleep 0.5; shot splash-0
sleep 1.2; shot splash-1
sleep 2.0; shot splash-2
wait_first_frame
shot desktop
goto 40 15; click; sleep 1; shot apps-overlay
key esc; sleep 0.6
goto 40 15; rclick; sleep 0.8; shot sysmenu
key esc; sleep 0.4
dock_icon settings; click; sleep 1.6
goto 320 $(( 115 + 36 * 9 )); click; sleep 1; shot settings-about
key ctrl-w; sleep 0.8
dock_icon browser; click; sleep 1.5
key ctrl-l; typestr "kitsune://sobre"; key ret; sleep 2; shot web-about
finish
