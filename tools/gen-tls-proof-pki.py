#!/usr/bin/env python3
"""Throw-away PKI for proving the browser's certificate errors in QEMU.

    python3 tools/gen-tls-proof-pki.py <outdir>

Writes, per scenario, `<name>.chain.pem` (leaf first, then intermediates) and
`<name>.key.pem` for an `openssl s_server`, plus `root.pem` (the proof root; add
it to a DEV trust store with `EXTRA_PEM=<outdir>/root.pem tools/gen-trust-store.sh`,
never commit that store). The guest reaches the host as 10.0.2.2 (SLIRP), so the
certificates name that IP.

Scenarios (port in the name list printed at the end):
  valid       chain to the proof root, IP SAN 10.0.2.2
  expired     same, but valid only during 2020
  wrongname   valid chain, but the SAN is only DNS:other.example
  selfsigned  a self-signed leaf with IP SAN 10.0.2.2
  untrusted   chain to a root that is NOT in the store
  future      valid only from 2031 (what a guest with a too-old clock sees)
"""
import datetime as dt
import ipaddress
import os
import sys

from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec
from cryptography.x509.oid import ExtendedKeyUsageOID, NameOID

UTC = dt.timezone.utc
out = sys.argv[1]
os.makedirs(out, exist_ok=True)


def name(cn):
    return x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, cn)])


def key():
    return ec.generate_private_key(ec.SECP256R1())


def cert(subject, k, issuer, ik, *, ca=False, pathlen=None, sans=None,
         nb=dt.datetime(2026, 1, 1, tzinfo=UTC), na=dt.datetime(2035, 1, 1, tzinfo=UTC)):
    b = (x509.CertificateBuilder().subject_name(name(subject)).issuer_name(name(issuer))
         .public_key(k.public_key()).serial_number(x509.random_serial_number())
         .not_valid_before(nb).not_valid_after(na)
         .add_extension(x509.BasicConstraints(ca=ca, path_length=pathlen), critical=True))
    if ca:
        b = b.add_extension(x509.KeyUsage(
            digital_signature=True, content_commitment=False, key_encipherment=False,
            data_encipherment=False, key_agreement=False, key_cert_sign=True, crl_sign=True,
            encipher_only=False, decipher_only=False), critical=True)
    if sans:
        b = b.add_extension(x509.SubjectAlternativeName(sans), critical=False)
        b = b.add_extension(x509.ExtendedKeyUsage([ExtendedKeyUsageOID.SERVER_AUTH]), critical=False)
    return b.sign(ik, hashes.SHA256())


def pem(c):
    return c.public_bytes(serialization.Encoding.PEM)


def write(n, chain, k):
    open(f"{out}/{n}.chain.pem", "wb").write(b"".join(pem(c) for c in chain))
    # `openssl s_server -cert` sends only the first certificate: give it the
    # leaf and the rest (intermediates) separately for -cert_chain.
    open(f"{out}/{n}.leaf.pem", "wb").write(pem(chain[0]))
    if len(chain) > 1:
        open(f"{out}/{n}.rest.pem", "wb").write(b"".join(pem(c) for c in chain[1:]))
    open(f"{out}/{n}.key.pem", "wb").write(k.private_bytes(
        serialization.Encoding.PEM, serialization.PrivateFormat.PKCS8,
        serialization.NoEncryption()))


IP = [x509.IPAddress(ipaddress.ip_address("10.0.2.2"))]
rk, ik = key(), key()
root = cert("OSjeff Proof Root", rk, "OSjeff Proof Root", rk, ca=True)
inter = cert("OSjeff Proof Intermediate", ik, "OSjeff Proof Root", rk, ca=True, pathlen=0)
open(f"{out}/root.pem", "wb").write(pem(root))

lk = key()
write("valid", [cert("proof", lk, "OSjeff Proof Intermediate", ik, sans=IP), inter], lk)
lk = key()
write("expired", [cert("proof", lk, "OSjeff Proof Intermediate", ik, sans=IP,
                       nb=dt.datetime(2020, 1, 1, tzinfo=UTC), na=dt.datetime(2020, 12, 31, tzinfo=UTC)), inter], lk)
lk = key()
write("future", [cert("proof", lk, "OSjeff Proof Intermediate", ik, sans=IP,
                      nb=dt.datetime(2031, 1, 1, tzinfo=UTC), na=dt.datetime(2032, 1, 1, tzinfo=UTC)), inter], lk)
lk = key()
write("wrongname", [cert("proof", lk, "OSjeff Proof Intermediate", ik,
                         sans=[x509.DNSName("other.example")]), inter], lk)
lk = key()
write("selfsigned", [cert("proof", lk, "proof", lk, sans=IP)], lk)
uk, uik, ulk = key(), key(), key()
uroot = cert("Untrusted Proof Root", uk, "Untrusted Proof Root", uk, ca=True)
uinter = cert("Untrusted Proof Inter", uik, "Untrusted Proof Root", uk, ca=True, pathlen=0)
write("untrusted", [cert("proof", ulk, "Untrusted Proof Inter", uik, sans=IP), uinter, uroot], ulk)

ports = {"valid": 4431, "expired": 4432, "wrongname": 4433, "selfsigned": 4434,
         "untrusted": 4435, "future": 4436}
for n, p in ports.items():
    rest = f" -cert_chain {out}/{n}.rest.pem" if os.path.exists(f"{out}/{n}.rest.pem") else ""
    print(f"openssl s_server -accept {p} -cert {out}/{n}.leaf.pem{rest} -key {out}/{n}.key.pem -www -quiet")
