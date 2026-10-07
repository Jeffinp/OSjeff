# Screenshots of the running desktop for docs/img (UEFI 1280x800 looks best).
source "$(dirname "$0")/../lib.sh"
wait_first_frame
sleep 2
shot 01-desktop
dock 613; click; sleep 4          # Task manager
shot 02-taskmgr
dock 829; click; sleep 4          # Files
shot 03-files
dock 721; click; sleep 5          # Browser
shot 04-browser
finish
