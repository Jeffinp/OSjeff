#!/usr/bin/env python3
"""Tiny classic-pcap summariser (Ethernet): ARP, DHCP, DNS, TCP flags, ICMP.

Reads the capture that tools/qemu-headless.sh writes to <outdir>/net.pcap and prints
one line per frame with the time since the first one, e.g.

     4 +  0.001s DHCP ACK 10.0.2.2:67 -> 255.255.255.255:68 yiaddr=10.0.2.15
     6 + 10.679s DHCP REQUEST 10.0.2.15:68 -> 10.0.2.2:67 yiaddr=0.0.0.0   (a unicast RENEW)

usage: python3 -I tools/pcapsum.py <outdir>/net.pcap   (-I: ignore the cwd and the environment)
"""
import struct
import sys

DHCP_T = {1: "DISCOVER", 2: "OFFER", 3: "REQUEST", 4: "DECLINE", 5: "ACK", 6: "NAK", 8: "INFORM"}


def ip(b):
    return ".".join(str(x) for x in b)


def mac(b):
    return ":".join("%02x" % x for x in b)


def dns_name(d):
    i, parts = 12, []
    while i < len(d) and d[i]:
        n = d[i]
        parts.append(d[i + 1 : i + 1 + n].decode("latin1"))
        i += n + 1
    return ".".join(parts)


def summarize(f):
    if len(f) < 14:
        return "short"
    et = struct.unpack("!H", f[12:14])[0]
    p = f[14:]
    if et == 0x0806 and len(p) >= 28:
        op = struct.unpack("!H", p[6:8])[0]
        sha, spa, tha, tpa = mac(p[8:14]), ip(p[14:18]), mac(p[18:24]), ip(p[24:28])
        if op == 1:
            kind = "gratuitous" if spa == tpa else "who-has"
            return f"ARP {kind} {tpa} tell {spa} ({sha})"
        return f"ARP reply {spa} is-at {sha}"
    if et == 0x0800 and len(p) >= 20:
        ihl = (p[0] & 15) * 4
        proto, src, dst = p[9], ip(p[12:16]), ip(p[16:20])
        l4 = p[ihl:]
        if proto == 17 and len(l4) >= 8:
            sp, dp = struct.unpack("!HH", l4[:4])
            d = l4[8:]
            if 67 in (sp, dp) and len(d) > 240:
                typ = "?"
                i = 240
                while i + 1 < len(d) and d[i] != 255:
                    if d[i] == 0:
                        i += 1
                        continue
                    if d[i] == 53:
                        typ = DHCP_T.get(d[i + 2], d[i + 2])
                    i += 2 + d[i + 1]
                yi = ip(d[16:20])
                return f"DHCP {typ} {src}:{sp} -> {dst}:{dp} yiaddr={yi}"
            if 53 in (sp, dp) and len(d) >= 12:
                kind = "response" if d[2] & 0x80 else "query"
                return f"DNS {kind} {src}:{sp} -> {dst}:{dp} {dns_name(d)}"
            return f"UDP {src}:{sp} -> {dst}:{dp} len={len(d)}"
        if proto == 6 and len(l4) >= 20:
            sp, dp = struct.unpack("!HH", l4[:4])
            fl = l4[13]
            names = [n for b, n in ((2, "SYN"), (1, "FIN"), (4, "RST"), (8, "PSH"), (16, "ACK")) if fl & b]
            return f"TCP {src}:{sp} -> {dst}:{dp} [{','.join(names)}]"
        if proto == 1:
            t = l4[0] if l4 else None
            names = {0: "echo reply", 3: "unreachable", 8: "echo request", 11: "time exceeded"}
            return f"ICMP {names.get(t, 'type=%s' % t)} {src} -> {dst}"
        return f"IP proto={proto} {src} -> {dst}"
    return f"ethertype 0x{et:04x}"


def main():
    data = open(sys.argv[1], "rb").read()
    magic = data[:4]
    end = "<" if magic == b"\xd4\xc3\xb2\xa1" else ">"
    off = 24
    n = 0
    t0 = None
    while off + 16 <= len(data):
        sec, usec, incl, _ = struct.unpack(end + "IIII", data[off : off + 16])
        off += 16
        fr = data[off : off + incl]
        off += incl
        t = sec + usec / 1e6
        t0 = t if t0 is None else t0
        n += 1
        print(f"{n:4d} +{t - t0:7.3f}s {summarize(fr)}")


main()
