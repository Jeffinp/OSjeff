# W26 top panel: Apps, Busca, the centred clock, the status pill; the calendar and notification
# centre (centred under the clock), Quick Settings (under the pill) and the system menu (right click
# on Apps). The notification list is empty on a quiet boot: a build with a temporary hook that logs
# a WARN and an ERROR a few seconds in fills it. Light: QEMU_EXTRA="-rtc base=2026-10-08T12:00:00".
source "$(dirname "$0")/../lib.sh"
wait_first_frame
shot panel-desktop
dock_icon files; click; sleep 2
goto 640 15; click; sleep 0.8; shot panel-centre
key esc; sleep 0.5
goto 1230 15; sleep 0.3; shot panel-pill-hover
click; sleep 0.8
goto 1100 150; sleep 0.3; shot panel-quick
# Aparência tile cycles Automática -> Clara -> Escura
goto 1190 120; click; sleep 0.8; shot panel-quick-appearance
key esc; sleep 0.5
goto 40 15; click; sleep 1; shot panel-apps
key esc; sleep 0.6
goto 40 15; rclick; sleep 0.8; shot panel-sysmenu
key esc; sleep 0.4
finish
