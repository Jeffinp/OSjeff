# i18n W32: toasts raised with a catalog text (`notify::notify_key`) in Portuguese and, after the
# switch in Ajustes, in English. Needs a TEMPORARY build hook (never committed) that calls
# notify_key twice (a warning and an error) once per language, 16 s after boot.
# UEFI (1280x800). Shots: toast-pt-*, toast-en-*.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
until grep -aq "w32-toast fired lang=0" "$OUT/serial.log" 2>/dev/null; do sleep 0.1; done
sleep 0.8; snap toast-pt-1
sleep 1.4; snap toast-pt-2; flush_snaps
key ctrl-w; sleep 0.8
dock_icon settings; click; sleep 1.6
goto 320 295; click; sleep 0.9
goto 700 258; click; sleep 1.2
until grep -aq "w32-toast fired lang=1" "$OUT/serial.log" 2>/dev/null; do sleep 0.1; done
sleep 0.8; snap toast-en-1
sleep 1.4; snap toast-en-2; flush_snaps
finish
