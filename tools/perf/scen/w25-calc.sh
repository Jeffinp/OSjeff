# Calculadora: keyboard and mouse, history strip, percent, sign, memory, copy, hover.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
dock_icon calc; click; sleep 1.4
shot c0-open
typestr "12*3="; sleep 0.5; shot c1-times
typestr "200+10%"; sleep 0.4; shot c2-percent
key equal; sleep 0.3
goto 741 262; click; sleep 0.4        # M+
key backspace
typestr "9n"; sleep 0.4; shot c3-neg-mem
goto 667 449; sleep 0.6; shot c4-hover
mon "mouse_button 1"; sleep 0.3; shot c5-pressed; mon "mouse_button 0"; sleep 0.3
typestr "1234567890123"; sleep 0.4; shot c6-long
typestr "*999999999="; sleep 0.5; shot c7-error
goto 760 150; click; sleep 0.5; shot c8-copied
goto 1100 14; click; sleep 0.6; goto 953 188; click; sleep 0.6
goto 800 20; click; sleep 0.6
dock_icon calc; click; sleep 1.0
typestr "7*8="; sleep 0.5; shot c9-light
finish
