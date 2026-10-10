# Network smoke test after a dependency upgrade: plain HTTP and a TLS handshake through the shell.
# Needs `python3 -m http.server 8000` and `openssl s_server -accept 4443 ... -www` on the host.
source "$(dirname "$0")/../lib.sh"
wait_first_frame
goto 300 250; click; sleep 0.5
typestr "curl http://10.0.2.2:8000/"; key ret; sleep 6
typestr "curl https://10.0.2.2:4443/"; key ret; sleep 10
shot net
finish
