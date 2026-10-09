# W23: the text editor: gutter, current line, selection, find / replace bar, status bar, the
# Open / Save-as sheet, the "save changes?" sheet and the Ctrl +/- text size. Disk as for w23-files.sh.
#   FS_IMG=<disk.img> QEMU_MEM=256M tools/perf/run.sh <img> bios <out> 200 tools/perf/scen/w23-editor.sh
source "$(dirname "$0")/../lib.sh"
wait_first_frame
dock_icon editor; click; sleep 2.5
snap e00-empty
typestr "fn main() {"; key ret
key tab; typestr "let total = 42;"; key ret
typestr "if total > 10 {"; key ret
key tab; typestr "show(total);"; key ret
key backspace; key backspace; key backspace; key backspace
typestr "}"; key ret
key backspace; key backspace; key backspace; key backspace
typestr "}"; key ret; key ret
typestr "fn show(n: i32) {"; key ret
key tab; typestr "print(n);"; key ret
key backspace; key backspace; key backspace; key backspace
typestr "}"
sleep 1
shot e01-typing
key shift-up; key shift-up; key shift-up; sleep 0.9
shot e02-selection
key ctrl-f; sleep 0.4; typestr "show"; sleep 0.9
shot e03-find
key esc; sleep 0.4
key ctrl-h; sleep 0.4; key tab; typestr "draw"; sleep 0.9
shot e04-replace
key esc; sleep 0.4
key ctrl-o; sleep 1.2
shot e05-open
key esc; sleep 0.6
key ctrl-equal; key ctrl-equal; key ctrl-equal; sleep 0.9
shot e06-zoom
key ctrl-0; sleep 0.5
key ctrl-shift-s; sleep 1.2
shot e07-saveas
typestr "nota"; key ret; sleep 1.5
shot e07b-saved
typestr "x"; sleep 0.3
key ctrl-q; sleep 1.2
shot e08-close
key esc; sleep 0.6
key ctrl-o; sleep 1; for i in 1 2 3 4 5 6 7; do key down; done; key ret; sleep 1.8
shot e09-opened
flush_snaps
finish
