#!/usr/bin/env python3
"""Generate the certificate chains used by kitsune_core's TLS verifier tests.

    python3 tools/gen-test-certs.py > kitsune_core/src/tlsverify/testcerts.rs

Needs the `cryptography` package. Every certificate is emitted as a hex string
constant; the tests decode them. Keys are throw-away (generated per run), so the
output differs between runs but the *scenarios* do not. Fixed reference time of
the tests: 2026-10-07T12:00:00Z.
"""
import datetime as dt
import sys

from cryptography import x509
from cryptography.hazmat.primitives import hashes, serialization
from cryptography.hazmat.primitives.asymmetric import ec, padding, rsa
from cryptography.x509.oid import ExtendedKeyUsageOID, NameOID

UTC = dt.timezone.utc
GOOD_FROM = dt.datetime(2026, 1, 1, tzinfo=UTC)
GOOD_TO = dt.datetime(2036, 1, 1, tzinfo=UTC)

out = []


def emit(name, data, doc=""):
    if doc:
        out.append(f"/// {doc}")
    out.append(f'pub const {name}: &str = "{data.hex()}";')


def rsa_key(bits=2048):
    return rsa.generate_private_key(public_exponent=65537, key_size=bits)


def ec_key(curve):
    return ec.generate_private_key(curve)


def name(cn):
    return x509.Name([x509.NameAttribute(NameOID.COMMON_NAME, cn)])


def make(subject_cn, key, issuer_cn, issuer_key, *, ca=False, pathlen=None, bc=True,
         sans=None, nb=GOOD_FROM, na=GOOD_TO, hash_alg=None, eku="server", ku=True,
         pss=False, serial=None):
    b = (x509.CertificateBuilder()
         .subject_name(name(subject_cn)).issuer_name(name(issuer_cn))
         .public_key(key.public_key())
         .serial_number(serial or x509.random_serial_number())
         .not_valid_before(nb).not_valid_after(na))
    if bc:
        b = b.add_extension(x509.BasicConstraints(ca=ca, path_length=pathlen), critical=True)
    if ku and ca:
        b = b.add_extension(x509.KeyUsage(
            digital_signature=True, content_commitment=False, key_encipherment=False,
            data_encipherment=False, key_agreement=False, key_cert_sign=True,
            crl_sign=True, encipher_only=False, decipher_only=False), critical=True)
    if sans:
        b = b.add_extension(x509.SubjectAlternativeName([x509.DNSName(s) for s in sans]),
                            critical=False)
    if eku == "server":
        b = b.add_extension(x509.ExtendedKeyUsage([ExtendedKeyUsageOID.SERVER_AUTH]),
                            critical=False)
    elif eku == "client":
        b = b.add_extension(x509.ExtendedKeyUsage([ExtendedKeyUsageOID.CLIENT_AUTH]),
                            critical=False)
    h = hash_alg or hashes.SHA256()
    if isinstance(issuer_key, rsa.RSAPrivateKey) and pss:
        # Sign with RSASSA-PSS (needs the cryptography >= 42 builder hook).
        cert = b.sign(issuer_key, h, rsa_padding=padding.PSS(padding.MGF1(h), h.digest_size))
    else:
        cert = b.sign(issuer_key, h)
    return cert


def der(c):
    return c.public_bytes(serialization.Encoding.DER)


# ---- RSA chain: root -> intermediate -> leaf ----
rr = rsa_key(); ri = rsa_key(); rl = rsa_key()
root = make("Test RSA Root", rr, "Test RSA Root", rr, ca=True)
inter = make("Test RSA Inter", ri, "Test RSA Root", rr, ca=True, pathlen=0)
leaf = make("rsa.test", rl, "Test RSA Inter", ri,
            sans=["rsa.test", "*.wild.test", "www.rsa.test"])
emit("RSA_ROOT", der(root), "RSA test root (self-signed CA).")
emit("RSA_INTER", der(inter), "RSA intermediate CA, pathLen 0.")
emit("RSA_LEAF", der(leaf), "Valid RSA leaf for rsa.test, www.rsa.test, *.wild.test.")

# SHA-384 / SHA-512 PKCS#1 variants
leaf384 = make("rsa384.test", rl, "Test RSA Inter", ri, sans=["rsa384.test"], hash_alg=hashes.SHA384())
leaf512 = make("rsa512.test", rl, "Test RSA Inter", ri, sans=["rsa512.test"], hash_alg=hashes.SHA512())
emit("RSA_LEAF_SHA384", der(leaf384))
emit("RSA_LEAF_SHA512", der(leaf512))

# Leaf signed with RSA-PSS
try:
    leaf_pss = make("pss.test", rl, "Test RSA Inter", ri, sans=["pss.test"], pss=True)
    emit("RSA_LEAF_PSS", der(leaf_pss), "Leaf whose certificate signature is RSASSA-PSS.")
except Exception as e:  # pragma: no cover
    print(f"// PSS leaf skipped: {e}", file=sys.stderr)

# ---- EC chain: P-256 root -> P-384 intermediate -> P-384 leaf, and P-256 leaf ----
er_ = ec_key(ec.SECP256R1()); ei = ec_key(ec.SECP384R1())
el384 = ec_key(ec.SECP384R1()); el256 = ec_key(ec.SECP256R1())
eroot = make("Test EC Root", er_, "Test EC Root", er_, ca=True, hash_alg=hashes.SHA256())
einter = make("Test EC Inter", ei, "Test EC Root", er_, ca=True, pathlen=0, hash_alg=hashes.SHA256())
eleaf384 = make("ec.test", el384, "Test EC Inter", ei, sans=["ec.test"], hash_alg=hashes.SHA384())
eleaf256 = make("ec256.test", el256, "Test EC Inter", ei, sans=["ec256.test"], hash_alg=hashes.SHA384())
emit("EC_ROOT", der(eroot), "P-256 root.")
emit("EC_INTER", der(einter), "P-384 intermediate signed by the P-256 root with SHA-256.")
emit("EC_LEAF384", der(eleaf384), "P-384 leaf for ec.test, ECDSA-SHA384 from the intermediate.")
emit("EC_LEAF256", der(eleaf256), "P-256 leaf for ec256.test.")

# ---- failure scenarios against the RSA chain ----
expired = make("expired.test", rl, "Test RSA Inter", ri, sans=["expired.test"],
               nb=dt.datetime(2019, 1, 1, tzinfo=UTC), na=dt.datetime(2020, 1, 1, tzinfo=UTC))
future = make("future.test", rl, "Test RSA Inter", ri, sans=["future.test"],
              nb=dt.datetime(2031, 1, 1, tzinfo=UTC), na=dt.datetime(2032, 1, 1, tzinfo=UTC))
emit("RSA_LEAF_EXPIRED", der(expired), "Leaf that expired in 2020.")
emit("RSA_LEAF_FUTURE", der(future), "Leaf valid only from 2031.")

selfsigned = make("self.test", rl, "self.test", rl, sans=["self.test"])
emit("SELF_SIGNED_LEAF", der(selfsigned), "Self-signed leaf (not a CA), self.test.")

# Untrusted root chain (a different CA that is NOT in the test store)
ur = rsa_key(); ul = rsa_key()
uroot = make("Untrusted Root", ur, "Untrusted Root", ur, ca=True)
uleaf = make("untrusted.test", ul, "Untrusted Root", ur, sans=["untrusted.test"])
emit("UNTRUSTED_ROOT", der(uroot), "A root that is not in the test trust store.")
emit("UNTRUSTED_LEAF", der(uleaf), "Leaf issued by the untrusted root.")

# Intermediate without basicConstraints
nbi = rsa_key(); nbl = rsa_key()
inter_nobc = make("No BC Inter", nbi, "Test RSA Root", rr, ca=True, bc=False)
leaf_nobc = make("nobc.test", nbl, "No BC Inter", nbi, sans=["nobc.test"])
emit("INTER_NO_BC", der(inter_nobc), "Intermediate with no basicConstraints extension.")
emit("LEAF_UNDER_NO_BC", der(leaf_nobc), "Leaf issued by the intermediate without basicConstraints.")

# Intermediate that is a CA=false certificate used as CA
nci = rsa_key(); ncl = rsa_key()
inter_notca = make("Not CA Inter", nci, "Test RSA Root", rr, ca=False)
leaf_notca = make("notca.test", ncl, "Not CA Inter", nci, sans=["notca.test"])
emit("INTER_NOT_CA", der(inter_notca), "Certificate with CA:FALSE used as an intermediate.")
emit("LEAF_UNDER_NOT_CA", der(leaf_notca))

# pathLen violation: root -> inter(pathlen 0) -> inter2(CA) -> leaf
i2 = rsa_key(); l2 = rsa_key()
inter2 = make("Deep Inter", i2, "Test RSA Inter", ri, ca=True)
leaf_deep = make("deep.test", l2, "Deep Inter", i2, sans=["deep.test"])
emit("INTER_DEEP", der(inter2), "CA below an intermediate whose pathLen is 0.")
emit("LEAF_DEEP", der(leaf_deep))

# clientAuth-only leaf
leaf_client = make("client.test", rl, "Test RSA Inter", ri, sans=["client.test"], eku="client")
emit("RSA_LEAF_CLIENT_EKU", der(leaf_client), "Leaf whose EKU is clientAuth only.")

# Tampered signature: flip the last byte of the (valid) leaf
tam = bytearray(der(leaf)); tam[-1] ^= 0x01
emit("RSA_LEAF_TAMPERED", bytes(tam), "RSA_LEAF with the last signature byte flipped.")
# Tampered TBS: change one byte of the subject name text
tam2 = bytearray(der(leaf)); i = bytes(tam2).find(b"rsa.test"); tam2[i] = ord("x")
emit("RSA_LEAF_TBS_EDIT", bytes(tam2), "RSA_LEAF with a TBS byte changed (signature no longer matches).")

# ---- TLS 1.3 CertificateVerify style signatures over a fixed message ----
MSG = b"OSjeff TLS 1.3 CertificateVerify test message"
emit("CV_MESSAGE", MSG)
emit("CV_RSA_PSS_SHA256", rl.sign(MSG, padding.PSS(padding.MGF1(hashes.SHA256()), 32), hashes.SHA256()),
     "RSA-PSS-SHA256 signature by RSA_LEAF's key.")
emit("CV_RSA_PSS_SHA384", rl.sign(MSG, padding.PSS(padding.MGF1(hashes.SHA384()), 48), hashes.SHA384()))
emit("CV_RSA_PKCS1_SHA256", rl.sign(MSG, padding.PKCS1v15(), hashes.SHA256()),
     "PKCS#1 v1.5 signature (forbidden in TLS 1.3 CertificateVerify).")
emit("CV_ECDSA_P256", el256.sign(MSG, ec.ECDSA(hashes.SHA256())), "ECDSA P-256 SHA-256 by EC_LEAF256's key.")
emit("CV_ECDSA_P384", el384.sign(MSG, ec.ECDSA(hashes.SHA384())), "ECDSA P-384 SHA-384 by EC_LEAF384's key.")

# Leaf with a 1024-bit RSA key (too weak) and its PSS signature
weak = rsa_key(1024)
leaf_weak = make("weak.test", weak, "Test RSA Inter", ri, sans=["weak.test"])
emit("RSA1024_LEAF", der(leaf_weak), "Leaf with a 1024-bit RSA key.")
emit("CV_RSA1024_PSS_SHA256", weak.sign(MSG, padding.PSS(padding.MGF1(hashes.SHA256()), 32), hashes.SHA256()))

print("// GENERATED by tools/gen-test-certs.py -- do not edit. Throw-away test keys.")
print("// Reference time for the tests: 2026-10-07T12:00:00Z.")
print()
print("\n".join(out))
