# W14: the read-only pages of the settings app (Rede, Armazenamento, Energia,
# Sobre) and the "renew DHCP" button (a no-op until the network front exposes the
# API). Coordinates are for the default window position on a 1280x720 screen.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
sleep 2
far() { goto "$1" "$2"; for _ in 1 2 3 4 5 6; do move 1 0; move -1 0; done; sleep 0.5; }
START_APPS=10
start_row() { # index -> click the start-panel row of app `index`
  local sy=$(( DOCKY - 20 - 12 - (20 + (START_APPS + 2) * 38 + 12) ))
  goto 451 $(( sy + 10 + 38 * $1 + 19 ))
  click
}
dock 451; click; sleep 1
start_row 8; sleep 2           # settings
far 320 257; click; sleep 1    # Rede
far 533 375; click; sleep 1    # Renovar DHCP
shot p1_network
far 320 295; click; sleep 1    # Armazenamento
shot p2_storage
far 320 333; click; sleep 1    # Energia
shot p3_power
far 320 371; click; sleep 1    # Sobre
shot p4_about
finish
