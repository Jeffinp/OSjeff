# W23: an idle desktop with the four redesigned apps open (Arquivos, Imagens, Terminal, Editor):
# after the carets stop blinking (12 s after the last input) no frame may be drawn.
#   QEMU_MEM=256M tools/perf/run.sh <img> bios <out> 120 tools/perf/scen/w23-idle.sh
# then count the frames per second of the last seconds with tools/perf/idle_tail.py <out>.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
dock_icon viewer; click; sleep 1.5
dock_icon files; click; sleep 1.5
dock_icon editor; click; sleep 1.5
goto 1000 650
sleep 28
finish
