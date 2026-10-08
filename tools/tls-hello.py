#!/usr/bin/env python3
"""List the TLS ClientHello random of every connection in a classic pcap (Ethernet, IPv4/TCP).

    python3 -I tools/tls-hello.py <outdir>/net.pcap   (-I: ignore the cwd and the environment)

Used to prove that the TLS client random differs between connections and boots
(docs/design/entropy.md section 5): tools/qemu-headless.sh writes the capture.
"""
import hashlib
import struct
import sys

d = open(sys.argv[1], "rb").read()
magic = struct.unpack("<I", d[:4])[0]
end = "<" if magic in (0xA1B2C3D4, 0xA1B23C4D) else ">"
off = 24
n = 0
while off + 16 <= len(d):
    ts, us, cl, ol = struct.unpack(end + "IIII", d[off:off + 16])
    f = d[off + 16:off + 16 + cl]
    off += 16 + cl
    if len(f) < 54 or f[12:14] != b"\x08\x00" or f[23] != 6:
        continue
    ihl = (f[14] & 15) * 4
    tcp = 14 + ihl
    doff = (f[tcp + 12] >> 4) * 4
    p = f[tcp + doff:]
    if len(p) > 43 and p[0] == 0x16 and p[5] == 1:
        rnd = p[11:43]
        n += 1
        dport = struct.unpack(">H", f[tcp + 2:tcp + 4])[0]
        sport = struct.unpack(">H", f[tcp:tcp + 2])[0]
        sid_len = p[43]
        print(f"hello {n}: t={ts}.{us:06d} sport={sport} dport={dport} random={rnd.hex()} sha256={hashlib.sha256(rnd).hexdigest()[:12]}")
