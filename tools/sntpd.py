#!/usr/bin/env python3
"""Tiny SNTP server for the QEMU proofs (the sandbox blocks outbound UDP/123).

    python3 tools/sntpd.py [--port 123] [--skew SECONDS] [--stratum 2]

The guest reaches it as its gateway (10.0.2.2:123 through SLIRP) when no public
time server answers. `--skew` adds an offset to the reported time, so a guest
whose RTC is wrong can be shown to correct itself. Replies are logged.
"""
import argparse
import socket
import struct
import time

NTP_UNIX = 2208988800


def ts(unix):
    secs = int(unix) + NTP_UNIX
    frac = int((unix - int(unix)) * (1 << 32))
    return struct.pack(">II", secs & 0xFFFFFFFF, frac)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--port", type=int, default=123)
    ap.add_argument("--skew", type=float, default=0.0)
    ap.add_argument("--stratum", type=int, default=2)
    ap.add_argument("--bind", default="0.0.0.0")
    a = ap.parse_args()
    s = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    s.bind((a.bind, a.port))
    print(f"sntpd: listening on {a.bind}:{a.port}, skew {a.skew}s", flush=True)
    while True:
        data, peer = s.recvfrom(512)
        now = time.time() + a.skew
        if len(data) < 48:
            continue
        origin = data[40:48]  # client's transmit timestamp
        pkt = bytearray(48)
        pkt[0] = (0 << 6) | (4 << 3) | 4  # LI 0, VN 4, mode 4 (server)
        pkt[1] = a.stratum
        pkt[2] = 6
        pkt[3] = 0xEC
        pkt[12:16] = b"LOCL"
        pkt[16:24] = ts(now - 5)  # reference
        pkt[24:32] = origin
        pkt[32:40] = ts(now)  # receive
        pkt[40:48] = ts(now)  # transmit
        s.sendto(bytes(pkt), peer)
        print(f"sntpd: answered {peer} at {time.strftime('%H:%M:%S', time.gmtime(now))} UTC", flush=True)


if __name__ == "__main__":
    main()
