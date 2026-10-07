# W15b smoke: the terminal at boot, a few commands, a screenshot after each.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
shot p-boot
typestr "echo hello world"; key ret; sleep 1.5
typestr "ls"; key ret; sleep 1.5
shot p-ls
typestr "cd Doc"; key tab; sleep 0.5
shot p-tab
key ret; sleep 1
typestr "pwd"; key ret; sleep 1
typestr "free"; key ret; sleep 1.5
shot p-end
finish
