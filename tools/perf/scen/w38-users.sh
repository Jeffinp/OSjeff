# W38: the Users page: create a second account with a password (the next boot, W39, asks who
# signs in). UEFI, 1280x800.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
dock_icon settings; click; sleep 1.6
goto 320 440; click; sleep 1.0
shot a-users
goto 825 402; click; typestr "maria"; key tab; typestr "Maria Silva"; key tab; typestr "senha1"; sleep 0.3
shot b-filled
goto 935 594; click; sleep 3
shot c-created
finish
