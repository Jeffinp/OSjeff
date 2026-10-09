# W26 own visual language: the six wallpaper presets (chosen in Configurações > Aparência), the
# flat icon tiles on the taskbar, the Apps grid, the pointer and the window borders.
# Chips of the wallpaper row (window at 220,84, 820x560): centres at y 277, x = 476 + 84 * i.
# Light: QEMU_EXTRA="-rtc base=2026-10-08T12:00:00" (only Crepúsculo changes with the appearance).
source "$(dirname "$0")/../lib.sh"
wait_first_frame
shot look-default
for i in 0 1 2 3 4 5; do
  dock_icon settings; click; sleep 1.6
  goto 270 145; click; sleep 0.6
  goto $((476 + 84 * i)) 277; click; sleep 1.2
  key ctrl-w; sleep 1.0
  goto 900 360; sleep 0.3
  shot look-wall$i
done
dock_icon apps; click; sleep 1.0; shot look-apps
key esc; sleep 0.8
goto 640 330; sleep 0.4; shot look-pointer
key ctrl-alt-g; sleep 1.2
goto 640 132; click; sleep 0.8; shot look-gallery-shell   # the Shell tab of the component gallery
finish
