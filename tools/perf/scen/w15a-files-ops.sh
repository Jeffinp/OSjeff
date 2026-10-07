# Scenario 3b: copy, rename, move, trash, restore and purge a 3 MB file.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
dock 829; click; sleep 3
key down; key down; key down; sleep 0.6      # big.bin
shot s3b-sel
key ctrl-c; sleep 0.5
key home; sleep 0.4; key ret; sleep 1.5      # into Documentos
key ctrl-v; sleep 1.2
shot s3b-copying
sleep 6
shot s3b-copied
key f2; sleep 1
key end; key dot; key c; sleep 0.5
shot s3b-rename
key ret; sleep 2.5
shot s3b-renamed
key ctrl-x; sleep 0.5
key backspace; sleep 1.5
key ctrl-v; sleep 3
shot s3b-moved
key delete; sleep 3
shot s3b-deleted
key tab; sleep 2
shot s3b-trash
key ret; sleep 3                              # Enter in the trash restores
key tab; sleep 2
shot s3b-restored
key end; sleep 0.5
key delete; sleep 3
key tab; sleep 1.5
key delete; sleep 1
shot s3b-confirm
key ret; sleep 3
shot s3b-purged
finish
