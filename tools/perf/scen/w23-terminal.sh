# W23: the terminal: tab strip, prompt colours, mouse selection and copy, overlay scrollbar,
# block / bar cursor, text size (Ctrl +/-) and the running pill. Disk as for w23-files.sh.
#   FS_IMG=<disk.img> QEMU_MEM=256M tools/perf/run.sh <img> bios <out> 200 tools/perf/scen/w23-terminal.sh
source "$(dirname "$0")/../lib.sh"
wait_first_frame
goto 300 260; click; sleep 0.5
typestr "ls /"; key ret; sleep 1.2
typestr "ls /Imagens"; key ret; sleep 1.2
typestr "echo ola mundo"
sleep 0.9
shot t01-prompt
key ret; sleep 0.8
# Drag across a line of output.
goto 90 196
mon "mouse_button 1"; sleep 0.08
move 160 22; sleep 0.1
mon "mouse_button 0"; sleep 0.3
CX=250; CY=218
shot t02-selection
# Double press selects a word; copy it and paste it on the live line.
goto 120 196; click; click; sleep 0.4
key ctrl-shift-c; sleep 0.3
key ctrl-v; sleep 0.8
shot t03-copied
key ctrl-u; typestr "seq 80"; key ret; sleep 1.5
mon "mouse_move 0 0 1"; mon "mouse_move 0 0 1"; sleep 0.4
shot t04-scrollback
key ctrl-end; sleep 0.3
key ctrl-equal; key ctrl-equal; key ctrl-equal; sleep 1
shot t05-zoom
key ctrl-0; sleep 0.6
typestr "sleep 4"; key ret; sleep 1.2
shot t06-running
key ctrl-c; sleep 1
flush_snaps
finish
