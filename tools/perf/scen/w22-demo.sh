# Frames for docs/img/demo.gif (BIOS, 1280x720): bar magnification, an app opening, typing,
# Busca, the calculator, the appearance switch, the Apps overlay.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
snap d01; sleep 0.3
dock_icon browser; sleep 0.5; snap d02
dock_icon editor; sleep 0.3; tap; snap d03; sleep 0.12; snap d04; sleep 1.0; snap d05
typestr "OSjeff"; key ret; typestr "um desktop de verdade"; sleep 0.4; snap d06
key ctrl-spc; sleep 0.5; typestr "cal"; sleep 0.6; snap d07
key ret; sleep 0.18; snap d08; sleep 0.9; snap d09
typestr "7*6"; key ret; sleep 0.4; snap d10
panel_item tray; click; sleep 0.7; snap d11
quick_tile appearance; click; sleep 0.7; snap d12
key esc; sleep 0.6; snap d13
dock_icon apps; click; sleep 1.0; snap d14
key esc; sleep 0.7; snap d15
flush_snaps
finish
