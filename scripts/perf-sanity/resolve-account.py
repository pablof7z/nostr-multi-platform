#!/usr/bin/env python3
"""Resolve an account entry from accounts.json to `<hex_pubkey> <nsec>`.

Accepts a name (the `name` key) OR a bare npub/hex. Decodes npub->hex via
bech32 (no external deps). Prints `HEX NSEC` on one line (NSEC blank if absent).

The harness NEVER commits real nsec to git — accounts.json is gitignored; this
template is illustrative. A SKIP-LOUD fixture-unavailable path covers the case
where no real high-follow account is provided.
"""
import json
import sys

CHARSET = "qpzry9x8gf2tvdw0s3jn54khce6mua7l"


def bech32_decode(bech):
    bech = bech.strip()
    pos = bech.rfind("1")
    if pos < 1:
        return None, None
    hrp = bech[:pos]
    data = []
    for c in bech[pos + 1:]:
        if c not in CHARSET:
            return None, None
        data.append(CHARSET.index(c))
    return hrp, data[:-6]  # strip the 6-char checksum


def convertbits(data, frombits, tobits):
    acc = 0
    bits = 0
    ret = []
    maxv = (1 << tobits) - 1
    for value in data:
        acc = (acc << frombits) | value
        bits += frombits
        while bits >= tobits:
            bits -= tobits
            ret.append((acc >> bits) & maxv)
    return ret


def npub_to_hex(npub):
    hrp, data = bech32_decode(npub)
    if hrp != "npub" or data is None:
        return None
    decoded = convertbits(data, 5, 8)
    return bytes(decoded[:32]).hex()


def main():
    if len(sys.argv) < 3:
        print("", "")
        return
    path, key = sys.argv[1], sys.argv[2]
    try:
        accounts = json.load(open(path)).get("accounts", [])
    except Exception:
        print("", "")
        return

    entry = None
    for a in accounts:
        if a.get("name") == key or a.get("npub") == key or a.get("hex") == key:
            entry = a
            break
    if entry is None and key.startswith("npub1"):
        entry = {"npub": key}

    if entry is None:
        print("", "")
        return

    hex_pk = entry.get("hex")
    if not hex_pk and entry.get("npub"):
        hex_pk = npub_to_hex(entry["npub"]) or ""
    nsec = entry.get("nsec", "")
    print(hex_pk or "", nsec or "")


if __name__ == "__main__":
    main()
