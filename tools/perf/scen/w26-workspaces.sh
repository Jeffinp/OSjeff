# W26 workspaces: Ctrl+Alt+Left/Right switch (the windows slide), the dots in the panel, a new window
# opens on the workspace on screen, Ctrl+Alt+Shift+arrows carry the focused window along, the title-bar
# menu offers "Mover para a área de trabalho N", clicking a dot goes there, and activating a window
# of another workspace (taskbar) brings its workspace.
# Light: QEMU_EXTRA="-rtc base=2026-10-08T12:00:00".
source "$(dirname "$0")/../lib.sh"
wait_first_frame
dock_icon files; click; sleep 1.5
shot ws-one
key ctrl-alt-right; sleep 0.12; snap ws-slide; flush_snaps; sleep 0.8
shot ws-two-empty
dock_icon editor; click; sleep 1.5
shot ws-two-editor
key ctrl-alt-left; sleep 1.0
shot ws-back-one
# carry the focused window (the editor is on workspace 2; go there and send it to 3)
key ctrl-alt-right; sleep 1.0
key ctrl-alt-shift-right; sleep 1.2
shot ws-moved
# the taskbar icon of the Arquivos window (workspace 1) brings workspace 1 back
dock_icon files; click; sleep 1.2
shot ws-follow
# click the third dot (dots start at x 130: 22, 8, 8 px wide, 6 px apart)
goto 176 15; click; sleep 1.0
shot ws-dot
finish
