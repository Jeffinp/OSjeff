# W8: apps follow their window. Maximize the editor (bigger text), the file
# manager and the browser, then open the dock context menu ("Nova janela").
# Work area (UEFI 1280x800): x 12..1268, y 76..708; its title-bar buttons sit at
# max x=1218..1236 (restore) y=82..100.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
far() { goto "$1" "$2"; for _ in 1 2 3 4 5 6; do move 1 0; move -1 0; done; sleep 0.7; }  # QEMU PS/2 sends >255 px jumps in 255 px packets, one per later input event: flush them
typew() { local s=$1 i ch; for ((i = 0; i < ${#s}; i++)); do ch=${s:i:1}; [ "$ch" = " " ] && ch=spc; key "$ch"; sleep 0.12; done; }

# editor: type, maximize with its button (610,110 560x350 -> max at 1120..1138,116..134)
dock 559; click; sleep 2
typew "text grows with the window"; key ret; typew "second line"
far 1129 125; sleep 0.5; click; sleep 1.5
shot w8_editor_max
far 1227 91; click; sleep 1.5

# files: (250,120 780x520 -> max at 980..998,126..144)
dock 829; click; sleep 2
far 989 135; sleep 0.5; click; sleep 1.5
shot w8_files_max
far 1227 91; click; sleep 1.5

# browser: (150,60 916x560 -> max at 1016..1034,66..84)
dock 721; click; sleep 2
far 1025 75; sleep 0.5; click; sleep 1.5
shot w8_browser_max
far 1227 91; click; sleep 1.5

# right click on the terminal's dock icon: "Nova janela"
dock 507; rclick; sleep 1.5
shot w8_dock_menu
# the menu pops up above the icon: item centre at y = DOCKY - 56
far 440 $(( DOCKY - 56 )); click; sleep 2
shot w8_new_window
finish
