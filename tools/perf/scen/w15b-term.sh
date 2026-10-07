# W15b terminal: history, Tab, Ctrl+L, a command that waits (the desktop stays alive and a second
# terminal works meanwhile), Ctrl+C, network commands, scrollback, and hostile output.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
typestr "echo one"; key ret; sleep 1
typestr "echo two"; key ret; sleep 1
key up; key up; sleep 0.4
shot t1-history                       # the line shows "echo one"
key ctrl-u; typestr "ls /D"; key tab; sleep 0.4
shot t1-tab                           # completes to /Documentos/
key ctrl-u
key ctrl-l; sleep 0.5
shot t2-ctrl-l
typestr "sleep 20"; key ret; sleep 1.5
shot t3-running                       # "executando..." and no prompt
key ctrl-n; sleep 1.5                 # a second terminal while the first one waits
typestr "echo second terminal works"; key ret; sleep 1.5
shot t3-second
key ctrl-w 2>/dev/null
goto 380 100; click; sleep 0.5        # focus the first terminal again (its title bar)
key ctrl-c; sleep 1.5
shot t4-ctrl-c                        # "sh: interrupted" and a prompt again
typestr "ping -c 2 10.0.2.2"; key ret; sleep 7
shot t5-ping
typestr "nslookup example.org"; key ret; sleep 6
typestr "nslookup 10.0.2.15"; key ret; sleep 1
typestr "ifconfig"; key ret; sleep 1.5
shot t5-net
typestr "curl -s http://10.0.2.2:9/"; key ret; sleep 6
shot t5-curl
typestr "seq 10000"; key ret; sleep 3
shot t6-seq
key pgup; key pgup; key pgup; sleep 0.6
shot t6-pgup
key ctrl-home; sleep 0.6
shot t6-top
key ctrl-end; sleep 0.4
typestr "f() { f; }; f"; key ret; sleep 3
shot t7-recursion
typestr "echo alive"; key ret; sleep 1.5
shot t7-alive
typestr "yes"; key ret; sleep 5
shot t8-yes                           # 1 MiB of "y", truncated, the desktop never froze
typestr "echo after-yes"; key ret; sleep 2
shot t8-after
finish
