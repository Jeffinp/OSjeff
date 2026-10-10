#!/usr/bin/env python3
"""A fake "public" HTTP/HTTPS server to test `net_http_get` of apps in QEMU.

The destination filter of apps refuses private addresses (`appnet::ipv4_allowed`), and
QEMU's SLIRP gateway is 10.0.2.2, so a test boots the guest on a public-looking user
network whose *host alias* is the address the app talks to. Connections the guest makes
to that address reach this server on the host's loopback:

    tools/nettest-server.py &                  # 127.0.0.1:8077 (HTTP), :8078 (HTTPS, self-signed)
    QEMU_NETDEV="user,id=n0,net=203.0.113.0/24,host=203.0.113.5,dhcpstart=203.0.113.15,dns=203.0.113.3" \\
      tools/perf/run.sh <img> bios <out> 150 tools/perf/scen/w18-net.sh

(203.0.113.0/24 is TEST-NET-3: a public range for the filter, never routed anywhere.)

HTTP routes (port 8077): /form (a POST form with select and textarea), /echo (shows the method
and body it got), /redir-post (303 -> /echo), /hello (200), /redir-ok (302 -> /hello), /redir-local (302 ->
http://10.0.2.2/, a private target the app policy must refuse), /missing (404), /gzip,
/chunked, /big (400 KiB, to see the cut). Port 8078 serves /hello over TLS with a
self-signed certificate (needs `openssl`): an app must NOT receive that page.

Every request line is appended to the file named by NETTEST_LOG (default: stderr).
"""
import gzip
import os
import socketserver
import ssl
import subprocess
import sys
import tempfile
import threading

LOG = os.environ.get("NETTEST_LOG")


def log(line):
    if LOG:
        with open(LOG, "a") as f:
            f.write(line + "\n")
    else:
        print(line, file=sys.stderr, flush=True)


class Handler(socketserver.StreamRequestHandler):
    def reply(self, status, headers, body=b""):
        head = f"HTTP/1.1 {status}\r\n" + "".join(f"{k}: {v}\r\n" for k, v in headers.items())
        self.wfile.write(head.encode() + b"\r\n" + body)

    def handle(self):
        headers = {}
        try:
            line = self.rfile.readline(4096).decode("latin-1", "replace").strip()
            while True:
                h = self.rfile.readline(4096)
                if h in (b"\r\n", b"\n", b""):
                    break
                k, _, v = h.decode("latin-1", "replace").partition(":")
                headers[k.strip().lower()] = v.strip()
            body_in = self.rfile.read(min(int(headers.get("content-length", "0") or 0), 1 << 20))
        except (OSError, ssl.SSLError, ValueError):
            return
        log(f"{self.server.server_address[1]} {line}")
        parts = line.split(" ")
        path = parts[1] if len(parts) > 1 else "/"
        close = {"Connection": "close"}
        if path == "/form":
            page = (
                "<html><head><title>Formulario</title></head><body><h1>Formulario</h1>"
                "<form method=post action=/echo>"
                "<p>Nome <input name=nome value=Ana></p>"
                "<p>Pais <select name=pais><option value=br>Brasil<option value=pt selected>Portugal"
                "<option value=ar>Argentina</select></p>"
                "<p>Mensagem<br><textarea name=msg rows=4 cols=30>Ola</textarea></p>"
                "<p><input type=submit value=Enviar></p></form>"
                "<form method=post action=/redir-post><input type=hidden name=x value=1>"
                "<input type=submit value='POST com redirect'></form></body></html>"
            ).encode()
            self.reply("200 OK", {"Content-Type": "text/html; charset=utf-8", "Content-Length": str(len(page)), **close}, page)
        elif path == "/echo":
            text = body_in.decode("utf-8", "replace")
            page = (
                "<html><head><title>Eco</title></head><body><h1>Recebido</h1>"
                f"<p>metodo: {parts[0]}</p><p>tipo: {headers.get('content-type', '-')}</p>"
                f"<p>tamanho: {headers.get('content-length', '-')}</p><pre>{text}</pre></body></html>"
            ).encode()
            self.reply("200 OK", {"Content-Type": "text/html; charset=utf-8", "Content-Length": str(len(page)), **close}, page)
        elif path == "/redir-post":
            self.reply("303 See Other", {"Location": "/echo", "Content-Length": "0", **close})
        elif path == "/hello":
            body = b"hello from the fake server"
            self.reply("200 OK", {"Content-Type": "text/plain", "Content-Length": str(len(body)), **close}, body)
        elif path == "/redir-ok":
            self.reply("302 Found", {"Location": "/hello", "Content-Length": "0", **close})
        elif path == "/redir-local":
            self.reply("302 Found", {"Location": "http://10.0.2.2/", "Content-Length": "0", **close})
        elif path == "/gzip":
            body = gzip.compress(b"gzip body, decoded by the OS", mtime=0)
            self.reply("200 OK", {"Content-Encoding": "gzip", "Content-Length": str(len(body)), **close}, body)
        elif path == "/chunked":
            body = b"6\r\nchunk1\r\n6\r\nchunk2\r\n0\r\n\r\n"
            self.reply("200 OK", {"Transfer-Encoding": "chunked", **close}, body)
        elif path == "/big":
            body = (b"0123456789abcdef" * 64 * 1024)[: 400 * 1024]
            self.reply("200 OK", {"Content-Length": str(len(body)), **close}, body)
        else:
            body = b"not found"
            self.reply("404 Not Found", {"Content-Length": str(len(body)), **close}, body)


class Server(socketserver.ThreadingTCPServer):
    allow_reuse_address = True
    daemon_threads = True


class TlsServer(Server):
    def get_request(self):
        sock, addr = super().get_request()
        try:
            return self.ctx.wrap_socket(sock, server_side=True), addr
        except (OSError, ssl.SSLError):
            sock.close()
            raise


def self_signed():
    d = tempfile.mkdtemp(prefix="nettest-")
    key, crt = os.path.join(d, "k.pem"), os.path.join(d, "c.pem")
    subprocess.run(
        ["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", key, "-out", crt,
         "-days", "2", "-subj", "/CN=203.0.113.5", "-addext", "subjectAltName=IP:203.0.113.5"],
        check=True, capture_output=True)
    ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    ctx.load_cert_chain(crt, key)
    return ctx


def main():
    http = Server(("127.0.0.1", 8077), Handler)
    threading.Thread(target=http.serve_forever, daemon=True).start()
    try:
        tls = TlsServer(("127.0.0.1", 8078), Handler)
        tls.ctx = self_signed()
        threading.Thread(target=tls.serve_forever, daemon=True).start()
        log("listening on 127.0.0.1:8077 (http) and :8078 (https, self-signed)")
    except Exception as e:  # no openssl: HTTP only
        log(f"listening on 127.0.0.1:8077 (http); no TLS server: {e}")
    threading.Event().wait()


if __name__ == "__main__":
    main()
