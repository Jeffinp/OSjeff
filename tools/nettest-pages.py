#!/usr/bin/env python3
"""A fake "public" web server of awkward compressed pages, to test the browser in QEMU (W20).

Real sites gzip pages that do not fit the response cap (`browser::MAX_RESPONSE_BYTES`, 1 MiB
on the wire); a stream cut in the middle used to show "Falha ao descompactar a pagina". The
browser must render the decoded prefix with a notice instead. Same setup as
tools/nettest-server.py (a public-looking SLIRP range whose host alias is this server):

    tools/nettest-pages.py &                  # 127.0.0.1:8077 (HTTP)
    QEMU_NETDEV="user,id=n0,net=203.0.113.0/24,host=203.0.113.5,dhcpstart=203.0.113.15,dns=203.0.113.3" \\
      tools/perf/run.sh <img> bios <out> 200 tools/perf/scen/w20-browser.sh

Every page is HTML whose <h1> names the case and whose last <p> says "END OF PAGE": a page that
shows the END line was decoded completely.

Routes: /plain (1.3 MB, identity: the byte cap cuts it), /gz-small, /gz-big (3.5 MB of
HTML, 1.2 MB gzipped: cut inside the gzip stream by the cap), /gz-huge (6 MB decoded, tiny on
the wire: the decoded limit), /gz-chunked (chunked + gzip, whole), /gz-chunked-big (chunked +
gzip, cut), /deflate (zlib), /deflate-raw, /gz-badcrc (a wrong CRC in the trailer), /gz-cut (the
server stops half way through the gzip body: Content-Length promises more), /gz-twice
(Content-Encoding: gzip, gzip), /GZ-CASE (Content-Encoding: GZIP), /br (claims brotli),
/gz-nolen (gzip, no Content-Length, closes the socket).

The log of request lines (including the Accept-Encoding the guest offered) goes to NETTEST_LOG
(default stderr).
"""
import gzip
import os
import random
import socketserver
import sys
import threading
import zlib

LOG = os.environ.get("NETTEST_LOG")


def log(line):
    if LOG:
        with open(LOG, "a") as f:
            f.write(line + "\n")
    else:
        print(line, file=sys.stderr, flush=True)


def html(title, kib, rich=True):
    """About `kib` KiB of HTML. `rich` text has 32 random bytes per line: it compresses only ~2.7:1."""
    out = [f"<html><head><title>{title}</title></head><body><h1>{title}</h1>\n"]
    n, i = 0, 0
    rng = random.Random(20)  # the same bytes every run
    while n < kib * 1024:
        junk = "%064x" % rng.getrandbits(256) if rich else "x" * 24
        line = f"<p>line {i}: {junk} lorem ipsum dolor sit amet</p>\n"
        out.append(line)
        n += len(line)
        i += 1
    out.append("<p>END OF PAGE</p></body></html>\n")
    return "".join(out).encode()


def raw_deflate(data):
    c = zlib.compressobj(6, zlib.DEFLATED, -15)
    return c.compress(data) + c.flush()


def chunked(body, size=7000):
    out = b""
    for i in range(0, len(body), size):
        c = body[i : i + size]
        out += f"{len(c):x}\r\n".encode() + c + b"\r\n"
    return out + b"0\r\n\r\n"


class Handler(socketserver.StreamRequestHandler):
    def send(self, headers, body, length=None):
        head = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nConnection: close\r\n"
        head += "".join(f"{k}: {v}\r\n" for k, v in headers.items())
        if length is not None:
            head += f"Content-Length: {length}\r\n"
        self.wfile.write(head.encode() + b"\r\n" + body)

    def handle(self):
        try:
            line = self.rfile.readline(4096).decode("latin-1", "replace").strip()
            offered = ""
            while True:
                h = self.rfile.readline(4096)
                if h in (b"\r\n", b"\n", b""):
                    break
                if h.lower().startswith(b"accept-encoding"):
                    offered = h.decode("latin-1").strip()
        except OSError:
            return
        log(f"{line} [{offered}]")
        path = (line.split(" ") + ["", "/"])[1]
        gz = lambda b: gzip.compress(b, 6, mtime=0)
        try:
            if path == "/plain":
                b = html("plain 1.3 MB", 1300)
                self.send({}, b, len(b))
            elif path == "/gz-small":
                b = gz(html("gz small", 60))
                self.send({"Content-Encoding": "gzip"}, b, len(b))
            elif path == "/gz-big":
                b = gz(html("gz big (cut by the cap)", 3500))
                self.send({"Content-Encoding": "gzip"}, b, len(b))
            elif path == "/gz-huge":
                b = gz(html("gz huge (decoded limit)", 6000, rich=False))
                self.send({"Content-Encoding": "gzip"}, b, len(b))
            elif path == "/gz-chunked":
                b = chunked(gz(html("gz chunked", 300)))
                self.send({"Transfer-Encoding": "chunked", "Content-Encoding": "gzip"}, b)
            elif path == "/gz-chunked-big":
                b = chunked(gz(html("gz chunked big (cut by the cap)", 3500)))
                self.send({"Transfer-Encoding": "chunked", "Content-Encoding": "gzip"}, b)
            elif path == "/deflate":
                b = zlib.compress(html("deflate (zlib)", 200))
                self.send({"Content-Encoding": "deflate"}, b, len(b))
            elif path == "/deflate-raw":
                b = raw_deflate(html("deflate (raw)", 200))
                self.send({"Content-Encoding": "deflate"}, b, len(b))
            elif path == "/gz-badcrc":
                b = bytearray(gz(html("gz bad crc", 80)))
                b[-8] ^= 0xFF
                self.send({"Content-Encoding": "gzip"}, bytes(b), len(b))
            elif path == "/gz-cut":
                b = gz(html("gz cut by the server", 400, rich=True))
                self.send({"Content-Encoding": "gzip"}, b[: len(b) // 2], len(b))
            elif path == "/gz-twice":
                b = gz(gz(html("gzip twice", 100)))
                self.send({"Content-Encoding": "gzip, gzip"}, b, len(b))
            elif path == "/GZ-CASE":
                b = gz(html("Content-Encoding: GZIP", 100))
                self.send({"content-encoding": "GZIP"}, b, len(b))
            elif path == "/br":
                self.send({"Content-Encoding": "br"}, b"\x0b\x02\x80binary", 9)
            elif path == "/gz-nolen":
                self.send({"Content-Encoding": "gzip"}, gz(html("gz without length", 150)))
            else:
                body = b"<h1>nettest-pages</h1><p>try /gz-big /gz-chunked /deflate ...</p>"
                self.send({}, body, len(body))
        except OSError:
            pass  # the guest closed early (it stops reading at its cap)


class Server(socketserver.ThreadingTCPServer):
    allow_reuse_address = True
    daemon_threads = True


def main():
    srv = Server(("127.0.0.1", int(os.environ.get("PORT", "8077"))), Handler)
    threading.Thread(target=srv.serve_forever, daemon=True).start()
    log("listening on 127.0.0.1:%d" % srv.server_address[1])
    threading.Event().wait()


if __name__ == "__main__":
    main()
