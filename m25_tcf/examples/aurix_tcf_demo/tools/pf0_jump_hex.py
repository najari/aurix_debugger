"""Intel HEX files for PF0 sector 0 (0x80000000, 16 KiB) from its page aware backup, for reset_demo.

    python pf0_jump_hex.py <backup>/sector_a0000000.json <out dir>

The backup is the JSON of pflash_check / the M25 flasher: { base, size, blank (per 32 B page), b64 }.

Only the programmed pages of the backup go into the files, so the erased pages stay erased.
  pf0_restore.hex  the backup as it is
  pf0_patch.hex    the same with the 4 bytes at 0x80000020 replaced by `j 0x8021C000`
Then both files are read back and compared with the backup.
"""
import base64
import json
import sys

PAGE = 32
BASE = 0x80000000
JUMP = bytes([0x1D, 0x10, 0xF0, 0xDF])  # j 0x8021C000, at 0x80000020
ORIGINAL_JUMP = bytes([0x9D, 0x81, 0xCD, 0xE6])  # ja 0x8003CD9A

backup, out_dir = sys.argv[1], sys.argv[2]
b = json.load(open(backup))
data = base64.b64decode(b["b64"])
blank = b["blank"]
assert b["base"] == 0xA0000000 and b["size"] == len(data) == 0x4000 and len(blank) == 0x4000 // PAGE
assert data[0x20:0x24] == ORIGINAL_JUMP, data[0x20:0x24].hex()
pages = [i for i, e in enumerate(blank) if e == 0]


def record(kind, address, payload):
    body = bytes([len(payload), (address >> 8) & 0xFF, address & 0xFF, kind]) + payload
    return ":" + body.hex().upper() + "%02X" % ((-sum(body)) & 0xFF)


def write_hex(path, image):
    lines = [record(4, 0, (BASE >> 16).to_bytes(2, "big"))]
    for i in pages:
        offset = i * PAGE
        for half in (0, 16):
            lines.append(record(0, offset + half, image[offset + half:offset + half + 16]))
    lines.append(record(1, 0, b""))
    open(path, "w").write("\n".join(lines) + "\n")


def read_hex(path):
    upper, out = 0, {}
    for line in open(path):
        raw = bytes.fromhex(line.strip()[1:])
        assert sum(raw) & 0xFF == 0
        n, address, kind, payload = raw[0], int.from_bytes(raw[1:3], "big"), raw[3], raw[4:-1]
        assert len(payload) == n
        if kind == 4:
            upper = int.from_bytes(payload, "big") << 16
        elif kind == 0:
            for j, byte in enumerate(payload):
                out[upper + address + j] = byte
    return out


patched = bytearray(data)
patched[0x20:0x24] = JUMP
write_hex(out_dir + "/pf0_restore.hex", data)
write_hex(out_dir + "/pf0_patch.hex", bytes(patched))

for name, image in (("pf0_restore.hex", data), ("pf0_patch.hex", bytes(patched))):
    got = read_hex(out_dir + "/" + name)
    expected_addresses = {BASE + i * PAGE + k for i in pages for k in range(PAGE)}
    assert set(got) == expected_addresses, name
    diff = [a for a in got if got[a] != image[a - BASE]]
    assert not diff, name
    vs_backup = sorted(a for a in got if got[a] != data[a - BASE])
    print(f"{name}: {len(pages)} pages, {len(got)} bytes, differs from the backup at "
          f"{[hex(a) for a in vs_backup] or 'nothing'}")
print("BMHD", data[:32].hex())
