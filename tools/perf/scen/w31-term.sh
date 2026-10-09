# i18n wave 31: the Terminal in Portuguese, then (after switching in Ajustes) in English.
# `help`, a missing path, an unknown command, df, free, ps, date, ping, a loop script. Run in UEFI
# (1280x800), US layout. Shots: pt-term-*, lang-en, en-term-*.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
term_session() { # <tag>: the terminal window is focused
  local t=$1
  goto 610 96; click; sleep 1.0          # maximize: more rows
  typestr "help | head -n 24"; key ret; sleep 1.2
  shot "$t-term-help"
  key ctrl-l; sleep 0.5
  typestr "ls /nonexistent"; key ret; sleep 0.8
  typestr "foo"; key ret; sleep 0.8
  typestr "mv a"; key ret; sleep 0.8
  typestr "kill abc"; key ret; sleep 0.8
  typestr "df"; key ret; sleep 0.8
  typestr "free"; key ret; sleep 0.8
  typestr "ps"; key ret; sleep 0.8
  shot "$t-term-a"
  key ctrl-l; sleep 0.5
  typestr "date"; key ret; sleep 0.8
  typestr "uptime"; key ret; sleep 0.8
  typestr "which ls"; key ret; sleep 0.8
  typestr "ping -c 1 10.0.2.2"; key ret; sleep 4
  typestr "ifconfig"; key ret; sleep 1
  typestr "nslookup 10.0.2.15"; key ret; sleep 1
  typestr "for i in 1 2 3; do echo item \$i; done"; key ret; sleep 1
  typestr "x=\$(seq 3 | wc -l); echo \$x lines"; key ret; sleep 1
  shot "$t-term-b"
}
term_session pt
# Ajustes > Idioma e região > English (same clicks as w28-lang.sh)
goto 800 400
dock_icon settings; click; sleep 1.4
goto 320 295; click; sleep 0.9
goto 700 258; click; sleep 1.2
key ctrl-w; sleep 0.8
key ctrl-w; sleep 0.8                    # the old terminal keeps what it printed: close it
dock_icon terminal; click; sleep 1.8     # a new one speaks English
shot lang-en
term_session en
finish
