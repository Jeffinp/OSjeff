# W37: accounts. First boot creates /etc/accounts and moves the old files into the user's home;
# the Terminal starts there and shows owners. UEFI, 1280x800.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
sleep 2
goto 300 300; click; sleep 0.5
typestr "whoami"; key ret; sleep 0.4
typestr "id"; key ret; sleep 0.4
typestr "pwd"; key ret; sleep 0.4
typestr "ls -l"; key ret; sleep 0.6
typestr "chmod 600 leiame.txt"; key ret; sleep 0.4
typestr "ls -l /home /"; key ret; sleep 0.8
shot a-terminal
typestr "clear"; key ret; sleep 0.3
typestr "chmod 000 notas.txt"; key ret; sleep 0.3
typestr "cat notas.txt"; key ret; sleep 0.5
typestr "touch /novo"; key ret; sleep 0.5
typestr "stat leiame.txt"; key ret; sleep 0.8
shot b-terminal
finish
