# Smoke test of the apps platform after a runtime upgrade: open each bundled app through Busca,
# let it run, and take a screenshot of it.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
open_app() { key ctrl-spc; sleep 1.2; typestr "$1"; sleep 0.8; key ret; sleep 3; }
open_app cobrinha; key right; sleep 1; key down; sleep 1; shot a-snake
open_app notas; typestr "ola"; sleep 1; shot b-notes
open_app pintura; sleep 2; shot c-paint
open_app relogio; sleep 2; shot d-clock
finish
