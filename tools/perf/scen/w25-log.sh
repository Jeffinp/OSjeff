# Registro: the table, a search, the level filter, the follow switch, wheel scrolling and
# the Salvar / Limpar buttons, light and dark.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
opened() { key ctrl-spc; sleep 0.6; typestr regis; sleep 0.6; key ret; sleep 1.6; }
wheel() { local d=$1 n=$2; for i in $(seq 1 $n); do mon "mouse_move 0 0 $d"; sleep 0.05; done; }
page() { # <tag>
  local t=$1
  shot "$t-all"
  typestr net; sleep 0.8; shot "$t-search"
  key backspace; key backspace; key backspace; sleep 0.4
  key tab; sleep 0.4; shot "$t-info"
  key tab; sleep 0.4; shot "$t-warn"
  key tab; key tab; sleep 0.4
  goto 600 300; wheel 1 8; sleep 0.8; shot "$t-scrolled"
  goto 660 126; click; sleep 0.6; shot "$t-follow-off"
  goto 870 112; click; sleep 1.0; shot "$t-saved"
  goto 790 112; click; sleep 1.0; shot "$t-cleared"
}
opened
page a
goto 1100 14; click; sleep 0.6; goto 953 188; click; sleep 0.6
goto 700 20; click; sleep 0.6
finish
