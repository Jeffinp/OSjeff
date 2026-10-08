# Window transitions: open (calculator), zoom (double-click the title), minimise, restore.
# Mid-flight screendumps land in $OUT/*.png (the click is a `tap`: the action starts on the press).
source "$(dirname "$0")/../lib.sh"
wait_first_frame
dock 666; tap                        # calculator
snap open-1
sleep 0.05; snap open-2
sleep 1.0; snap open-done
goto 620 165; click; tap             # double-click the title bar: zoom
snap zoom-1
sleep 0.06; snap zoom-2
sleep 1.2; snap zoom-done
goto 620 40; click; tap              # title bar of the zoomed window: zoom back
snap unzoom-1
sleep 1.2
goto 705 165; tap                    # minimise button
snap min-1
sleep 0.07; snap min-2
sleep 1.0; snap min-done
dock 666; tap                        # restore from the dock
snap restore-1
sleep 0.07; snap restore-2
sleep 1.0; snap restore-done
flush_snaps
finish
