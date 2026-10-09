# W26 window chrome: flat buttons at the right (hover fills, red close), the title-bar menu button,
# edge snapping with its animated preview (half, quarter, maximise), Alt+arrow moves and restoring
# a tiled window by dragging its title. Windows: Arquivos at (220,110,860,520), Editor at
# (610,110,560,350): buttons are 40x32 cells flush with the right edge (close = x+w-20).
# Light appearance: QEMU_EXTRA="-rtc base=2026-10-08T12:00:00" (Auto follows the guest clock).
source "$(dirname "$0")/../lib.sh"
wait_first_frame
dock_icon files; click; sleep 2
goto 1060 126; sleep 0.4; shot wm-hover-close
goto 1020 126; sleep 0.4; shot wm-hover-max
# drag the title to the left edge: preview, then drop = left half
goto 500 126; sleep 0.3
mon "mouse_button 1"; sleep 0.1
move -200 0; sleep 0.1; move -200 0; sleep 0.1; move -98 0; sleep 0.1; move 0 60; sleep 0.1; move 0 60; sleep 0.1
snap wm-preview-left; flush_snaps
mon "mouse_button 0"; sleep 1.5
shot wm-snapped-left
# up = top-left quarter, up again = maximised, down = restored
key alt-up; sleep 1.2; shot wm-quarter
key alt-up; sleep 1.2; shot wm-maximised
key alt-down; sleep 1.2; shot wm-restored
# the title menu of the editor (its menu button sits at x+w-120-16)
dock_icon editor; click; sleep 2
goto 1034 126; sleep 0.3; click; sleep 0.8; shot wm-menu
key esc; sleep 0.4
finish
