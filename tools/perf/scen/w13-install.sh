# W13 proof (c): install / remove an app from the Files "Apps" place and watch the
# Start panel follow (the Apps place returned in W18; see also w18-apps.sh). `A` opens
# it. Rows are in name order (Notas, Ola, Pintura, Plasma, Relogio, Snake); a removed
# app stays in the list as "nao instalado".
source "$(dirname "$0")/../lib.sh"
wait_first_frame
dock 829; click; sleep 1.5
key a; sleep 0.8
shot c1-files-apps
key down; sleep 0.3; key delete; sleep 1                # remove "Ola"
shot c2-removed
dock 451; click; sleep 1; key end; sleep 0.5; shot c3-start-without-ola
key esc; sleep 0.5
goto 640 400; click; sleep 0.5                           # refocus the Files window
key i; sleep 1                                           # install "Ola" again
shot c4-reinstalled
dock 451; click; sleep 1; key end; sleep 0.5; shot c5-start-with-ola
key esc; sleep 0.5
finish
