# W13 proof (c): install / remove an app from the Files "Apps" view and watch the
# Start panel follow. Files opens at (250,120); Tab cycles Files > Trash > boot > FS
# > Apps. Rows (name order): Notas, Ola, Pintura, Plasma, Relogio, Snake; a removed
# app moves to the end of the list as "nao instalado".
source "$(dirname "$0")/../lib.sh"
wait_first_frame
dock 829; click; sleep 1.5
key tab; key tab; key tab; key tab; sleep 0.8
shot c1-files-apps
key down; sleep 0.3; key delete; sleep 1                # remove "Ola"
shot c2-removed
dock 451; click; sleep 1; shot c3-start-without-ola
key esc; sleep 0.5
goto 640 400; click; sleep 0.5                           # refocus the Files window
for i in 1 2 3 4 5; do key down; sleep 0.2; done         # select the uninstalled "Ola"
key i; sleep 1                                           # install it again
shot c4-reinstalled
dock 451; click; sleep 1; shot c5-start-with-ola
key esc; sleep 0.5
finish
