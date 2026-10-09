# i18n W32: the Navegador in Portuguese and English: the start page, the osjeff:// pages, the tab
# strip, the find bar, the context menu, an unreachable site, a refused certificate and the
# connection popover; then the language is switched live in Ajustes and the same windows are shot
# again (error pages and internal pages are drawn again in the new language). UEFI (1280x800).
# Needs, on the host:
#   tools/nettest-server.py &     (8078: self-signed HTTPS)     python3 -m http.server-like page on 8080
# and the guest on the public-looking range:
#   QEMU_NETDEV="user,id=n0,net=203.0.113.0/24,host=203.0.113.5,dhcpstart=203.0.113.15,dns=203.0.113.3"
# The page on 8080 may log the Accept-Language header it receives (the browser sends the
# interface language: pt-BR,pt;q=0.9,en;q=0.8 or en;q=1).
# Shots: web-pt-*, web-en-*.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
key ctrl-w; sleep 0.8
dock_icon browser; click; sleep 2.5
shot web-pt-start
# tab 1: the certificate error; tab 2: an unreachable site; tab 3: the about page
key ctrl-l; typestr "https://203.0.113.5:8078/hello"; key ret; sleep 9
shot web-pt-cert
key ctrl-t; sleep 0.8
key ctrl-l; typestr "http://203.0.113.5:9/"; key ret; sleep 6
shot web-pt-refused
key ctrl-t; sleep 0.8
key ctrl-l; typestr "http://203.0.113.5:8080/hdr-pt"; key ret; sleep 5
shot web-pt-page
key ctrl-t; sleep 0.8
key ctrl-l; typestr "osjeff://sobre"; key ret; sleep 1.5
shot web-pt-about
goto 640 400; rclick; sleep 0.7; shot web-pt-context; key esc; sleep 0.5
key ctrl-f; sleep 0.4; typestr "aba"; sleep 0.6; shot web-pt-find; key esc; sleep 0.4
key ctrl-l; typestr "osjeff://historico"; key ret; sleep 1.2; shot web-pt-history
# the language switches while the browser has the error page and the internal page open
dock_icon settings; click; sleep 1.6
goto 320 295; click; sleep 0.9
goto 700 258; click; sleep 1.2
dock_icon browser; click; sleep 1.2
shot web-en-history
key ctrl-l; typestr "osjeff://sobre"; key ret; sleep 1.2; shot web-en-about
goto 640 400; rclick; sleep 0.7; shot web-en-context; key esc; sleep 0.5
key ctrl-f; sleep 0.4; for i in 1 2 3; do key backspace; done; typestr "tab"; sleep 0.6; shot web-en-find; key esc; sleep 0.4
key ctrl-1; sleep 1.0; shot web-en-cert
key ctrl-2; sleep 1.0; shot web-en-refused
key ctrl-t; sleep 0.8; shot web-en-start
key ctrl-l; typestr "http://203.0.113.5:8080/hdr-en"; key ret; sleep 5
shot web-en-page
finish
