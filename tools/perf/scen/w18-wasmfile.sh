# W18 proof (b2): a .wasm package as a file. /apps/<id>.wasm in the file manager:
# Properties shows the manifest (permissions, quotas) and whether it is installed;
# Enter on a package that is not installed installs and runs it (here: the app is
# removed from the Apps place first, then opened from its file).
source "$(dirname "$0")/../lib.sh"
wait_first_frame
dock 829; click; sleep 2
key home; key ret; sleep 1.2                             # root: the first folder is /apps
shot w1-apps-folder
goto 560 263; rclick; sleep 0.6                          # a package row
shot w2-menu
goto 600 438; click; sleep 1                             # Propriedades
shot w3-props
key esc; sleep 0.5
key a; sleep 1; key delete; sleep 1                      # Apps place: remove the first app (Notas)
key backspace; sleep 1; key backspace; sleep 1           # back to /apps (history)
shot w4-removed
finish
