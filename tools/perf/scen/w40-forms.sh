# W40: a POST form with <select> and <textarea> against tools/nettest-server.py (port 8077 on the
# host, 10.0.2.2 for the guest). UEFI, 1280x800.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
dock_icon browser; click; sleep 1.5
typestr "http://10.0.2.2:8077/form"; key ret; sleep 5
shot a-form
goto 255 283; click; sleep 0.5; click; sleep 0.5; click; sleep 0.5
goto 300 390; click; typestr " mundo"; key ret; typestr "linha dois"; sleep 0.5
shot b-filled
goto 193 457; click; sleep 4
shot c-echo
goto 178 116; click; sleep 4
goto 236 503; click; sleep 4
shot d-redirect
finish
