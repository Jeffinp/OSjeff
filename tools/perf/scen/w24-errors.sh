# W24: friendly error pages (certificate, DNS, offline path, timeout) and "continue anyway".
#   tools/nettest-server.py &   (8078: self-signed HTTPS)     tools/w24-site.py &   (8079)
# LIGHT=1 for the light appearance. bios 1280x720.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
t=dark
if [ "${LIGHT:-0}" = 1 ]; then
  t=light
  goto 1100 14; click; sleep 0.6; goto 953 188; click; sleep 0.6; key esc; sleep 0.6
fi
key ctrl-w; sleep 0.8
dock_icon browser; click; sleep 2
key ctrl-l; typestr "https://203.0.113.5:8078/hello"; key ret; sleep 9
shot "$t-e-cert"
goto 607 530; sleep 0.3
shot "$t-e-cert-hover"
goto 607 486; click; sleep 9
shot "$t-e-proceeded"
goto 340 116; click; sleep 0.8
shot "$t-e-popover"
key esc; sleep 0.3
key ctrl-l; typestr "http://nao-existe.invalid/"; key ret; sleep 8
shot "$t-e-dns"
key ctrl-l; typestr "http://203.0.113.5:8079/hang"; key ret; sleep 3
shot "$t-e-loading"
sleep ${HANG_WAIT:-25}
shot "$t-e-timeout"
finish
