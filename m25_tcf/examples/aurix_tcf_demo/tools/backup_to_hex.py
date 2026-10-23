"""Intel HEX file from PFLASH sector backups, to put sectors back with `probe-rs download`.

    python backup_to_hex.py <backup dir or sector_*.json> [...] <out.hex>

A backup is the JSON that probe-rs writes before it erases a sector (PROBE_RS_TRICORE_FLASH_BACKUP)
or that pflash_check compares with: { base, size, blank (1 per erased 32 B page), b64 }.

Only the programmed pages go into the file, at the cached address (0x8...). The TC27x flash
loader erases each sector the file touches and writes only these pages, so the erased pages of
the backup are erased again. A backup with no programmed page is left out (nothing to write;
erase that sector by other means if it has to be blank again).

    probe-rs download --chip TC275 --binary-format hex <out.hex>
"""
import base64
import json
import pathlib
import sys

PAGE = 32
UNCACHED_TO_CACHED = 0x20000000


def record(kind, address, payload):
    body = bytes([len(payload), (address >> 8) & 0xFF, address & 0xFF, kind]) + payload
    return ":" + body.hex().upper() + "%02X" % ((-sum(body)) & 0xFF)


def backups(args):
    for arg in args:
        path = pathlib.Path(arg)
        yield from sorted(path.glob("sector_*.json")) if path.is_dir() else [path]


def main():
    if len(sys.argv) < 3:
        sys.exit(__doc__)
    *inputs, out = sys.argv[1:]
    lines, upper = [], None
    for path in backups(inputs):
        b = json.loads(path.read_text())
        data = base64.b64decode(b["b64"])
        base = b["base"] - UNCACHED_TO_CACHED if b["base"] >= 0xA0000000 else b["base"]
        assert len(data) == b["size"] == len(b["blank"]) * PAGE, path
        pages = [i for i, blank in enumerate(b["blank"]) if blank == 0]
        print(f"{path.name}: {base:#010x}, {len(pages)} of {len(b['blank'])} pages programmed")
        for i in pages:
            for half in (0, 16):
                address = base + i * PAGE + half
                if address >> 16 != upper:
                    upper = address >> 16
                    lines.append(record(4, 0, upper.to_bytes(2, "big")))
                lines.append(record(0, address & 0xFFFF, data[i * PAGE + half:i * PAGE + half + 16]))
    lines.append(":00000001FF")
    pathlib.Path(out).write_text("\n".join(lines) + "\n")
    print(f"{out}: {len(lines) - 1} records")


if __name__ == "__main__":
    main()
