#!/usr/bin/env python3
"""Encrypt the fictional legacy-schema trees (legacy_rel.xml, legacy_old.xml) into old tree-part variants.

An implementation of the documented algorithms (docs/design.md) that is independent of the Rust codec:
    nix-shell -p 'python3.withPackages (p: [p.cryptography])' --run 'python3 make_vectors.py'
Output files are named <tree>.<part>[_vN]; the tests wrap them in a package under the matching part name.
"""
import hashlib, hmac, os, struct, zlib
from cryptography.hazmat.primitives.ciphers import Cipher, algorithms, modes

HERE = os.path.dirname(os.path.abspath(__file__))
PW7 = "rzy2z3AAOw4Hv2l1jZLq6BYcGVEWy3fS52rTebxVMPgE6SQDg1ONfHZnA6UyQvt"
PW4 = "A9426036-1CCF-427C-896C-33E05CAF829F"
PW1 = "0B0995E2-5285-4B85-86E2-E90EAEA7397C"
PW5 = "IxXqhSlqHxKvCniULelX9sDnXhqzdakEoFEcmJf6OYmnDfGZrIkb4RIA0wnu71P"
TAG_REST = bytes.fromhex("429A2662181453386A067D5BB8C02A5292CBDB15EE915892CE38BEFF2DF566")
TAGS = {1: b"\x3a" + TAG_REST, 2: b"\x5a" + TAG_REST, 3: b"\x7c" + TAG_REST}
PARAMS = {1: ("sha256", 10000, 256), 2: ("sha256", 10000, 32), 3: ("sha512", 210000, 32)}  # hash, iterations, master key length


def deflate(xml):
    c = zlib.compressobj(6, zlib.DEFLATED, -15)
    return c.compress(xml) + c.flush()


def cbc(key, iv, raw):
    p = 16 - len(raw) % 16
    return Cipher(algorithms.AES(key), modes.CBC(iv)).encryptor().update(raw + bytes([p]) * p)


def aes_part(xml, password, version):
    salt, iv = os.urandom(32), os.urandom(16)
    h, it, n = PARAMS[version]
    master = hashlib.pbkdf2_hmac(h, password.encode("utf-16-le"), salt, it, n)

    def kdf(suffix):
        return hmac.new(master, struct.pack(">I", 1) + b"ChronoplexSoftware\0xml" + suffix + struct.pack(">I", 256),
                        hashlib.sha512).digest()[:32]

    body = TAGS[version] + salt + iv + cbc(kdf(b"|encryption_key"), iv, deflate(xml))
    return hmac.new(kdf(b"|mac_key"), body, hashlib.sha512).digest() + body


def legacy_part(xml, password, iterations, fixed_salt=False):
    salt = password.encode() if fixed_salt else os.urandom(32)
    kiv = hashlib.pbkdf2_hmac("sha1", password.encode(), salt, iterations, 48)
    ct = cbc(kiv[:32], kiv[32:], deflate(xml))
    return ct if fixed_salt else os.urandom(32) + salt + ct


def write(name, data):
    open(os.path.join(HERE, name), "wb").write(data)


rel = open(os.path.join(HERE, "legacy_rel.xml"), "rb").read()
old = open(os.path.join(HERE, "legacy_old.xml"), "rb").read()
write("legacy_rel.family7_v2", aes_part(rel, PW7, 2))
write("legacy_rel.family5", legacy_part(rel, PW5, 10622))
write("legacy_old.family4", legacy_part(old, PW4, 10564))
write("legacy_old.family", legacy_part(old, PW1, 1258, fixed_salt=True))
