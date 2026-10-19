/*
 * m25_4b_flash.js - M25-4b: host driver for our own AURIX PFLASH flasher.
 *
 * Drives the M25 flash loader (runs in PSPR) through the mailbox in DSPR, using
 * the live TCF agent (TAS/DAS) for memory/register access.
 *
 * Device: detected from CBS_JTAGID (0xF0000464).  TC27x (JTAG ID x01DA083) uses
 * m25_flash_loader_tc27x.bin and the TC27x logical sector map (16..256 KiB,
 * from the Infineon AURIX Flasher DeviceConfigs/TC27x_D_step.json); set
 * M25_DEVICE=tc37x to use the TC37x loader (6 MiB, 16 KiB sectors).  Any other
 * ID is refused - the loaders write device specific SCU/flash registers.
 *
 * Usage:
 *   node m25_4b_flash.js device                    - show the detected device
 *   node m25_4b_flash.js save                      - save regs + PSPR/DSPR scratch -> state file
 *   node m25_4b_flash.js load                      - upload loader, run it, wait for magic
 *   node m25_4b_flash.js erase  <addr> [count]     - erase N logical sectors (addr = a sector start)
 *   node m25_4b_flash.js program <addr> <file>     - program file bytes at addr (32-byte pages)
 *   node m25_4b_flash.js read   <addr> <size> <out>- read flash to a file
 *   node m25_4b_flash.js verify <addr> <file>      - compare flash with a file
 *   node m25_4b_flash.js restore <file> <addr>     - erase + program a file back (restore)
 *   node m25_4b_flash.js stop                      - exit loader (ENDINIT back), halt core, restore state
 *
 * Safety: `save` must be run before `load`; `stop` restores registers and the
 * PSPR/DSPR scratch regions (the flash content is NOT restored - use backup/restore).
 */
'use strict';
const net = require('net');
const fs = require('fs');
const EOM = Buffer.from([3, 1]);

function tb(w, a) {
    const p = [Buffer.from(w.join('\0') + '\0', 'utf8')];
    for (const x of (a || [])) { p.push(Buffer.from(JSON.stringify(x), 'utf8')); p.push(Buffer.from([0])); }
    p.push(EOM);
    return Buffer.concat(p);
}
function splitZeros(buf) {
    const out = []; let s = 0;
    for (let i = 0; i < buf.length; i++) if (buf[i] === 0) { out.push(buf.subarray(s, i)); s = i + 1; }
    if (s < buf.length) out.push(buf.subarray(s));
    return out;
}
const pending = new Map(); let token = 0; let buf = Buffer.alloc(0);
const sock = net.connect(1534, '127.0.0.1', () => sock.write(tb(['E', 'Locator', 'Hello', JSON.stringify(['Locator'])])));
sock.on('data', (d) => {
    buf = Buffer.concat([buf, d]);
    for (;;) {
        const i = buf.indexOf(EOM);
        if (i < 0) return;
        const p = splitZeros(buf.subarray(0, i));
        buf = buf.subarray(i + 2);
        if (p[0].toString() !== 'R') continue;
        const pr = pending.get(p[1].toString());
        if (!pr) continue;
        pending.delete(p[1].toString());
        const err = p[2] ? p[2].toString() : '';
        if (pr.raw) pr.resolve(p.slice(2).map((x) => x.toString()));
        else if (err && err !== 'null') pr.reject(new Error('TCF error: ' + err));
        else pr.resolve(p.slice(3).map((x) => x.toString()));
    }
});
const cmd = (service, c, ...args) => new Promise((res, rej) => { const t = String(++token); pending.set(t, { resolve: res, reject: rej }); sock.write(tb(['C', t, service, c], args)); });
const cmdRaw = (service, c, ...args) => new Promise((res, rej) => { const t = String(++token); pending.set(t, { resolve: res, reject: rej, raw: true }); sock.write(tb(['C', t, service, c], args)); });
const j = (d) => (d && d.length ? JSON.parse(d[0]) : null);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const CORE = 'core0';
const PSPR = 0x70100000;
const MAILBOX = 0x7000F000;
const PAYLOAD = 0x7000A000;          /* DSPR staging buffer (8 KiB) */
const LOADER_SP = 0x7000E000;        /* stack: 0x7000C000-0x7000DFFF, above the payload */
const FLASHER_DIR = require('path').join(__dirname, '..', 'flasher');
const STATE_FILE = process.env.M25_STATE ||
    require('path').join(require('os').tmpdir(), 'm25_4b_state.json');

/* ---- device profiles ---- */
function sectorRuns(runs) {
    const list = [];
    for (const [start, size, count] of runs)
        for (let i = 0; i < count; i++) list.push({ base: (start + i * size) >>> 0, size });
    return list;
}
/* TC27x logical sectors (uncached addresses), PF0 and PF1 have the same layout */
const TC27X_PF = [[0x4000, 8], [0x8000, 8], [0x10000, 4], [0x20000, 3], [0x40000, 4]];
const DEVICES = {
    tc27x: {
        name: 'TC27x',
        loader: 'm25_flash_loader_tc27x.bin',
        pflashEnd: 0xA03FFFFF,
        sectors: (() => {
            const runs = [];
            for (const bank of [0xA0000000, 0xA0200000]) {
                let a = bank;
                for (const [size, count] of TC27X_PF) { runs.push([a, size, count]); a += size * count; }
            }
            return sectorRuns(runs);
        })(),
    },
    tc37x: {
        name: 'TC37x',
        loader: 'm25_flash_loader.bin',
        pflashEnd: 0xA05FFFFF,
        sectors: sectorRuns([[0xA0000000, 0x4000, 0x600000 / 0x4000]]),
    },
};
let DEV = null;
async function detectDevice() {
    const id = (await memRead(0xF0000464, 4)).readUInt32LE(0) >>> 0;
    let key = (process.env.M25_DEVICE || '').toLowerCase();
    if (!key && (id & 0x0FFFFFFF) === 0x001DA083) key = 'tc27x';
    if (!DEVICES[key]) throw new Error(`unknown device (JTAG ID 0x${id.toString(16)}) - set M25_DEVICE=tc27x|tc37x only if you are sure`);
    DEV = DEVICES[key];
    return id;
}
const sectorAt = (uncached) => DEV.sectors.find((s) => uncached >= s.base && uncached < s.base + s.size);

/* register index by name (Registers service children of the core) */
async function regIndex(name) {
    const ids = j(await cmd('Registers', 'getChildren', CORE)) || [];
    for (const id of ids) {
        const c = j(await cmd('Registers', 'getContext', id));
        if (c && c.Name === name) return Number(/^R(\d+)\./.exec(id)[1]);
    }
    throw new Error(`register ${name} not found`);
}
const MAGIC = 0x4D32354D;
const MB = { MAGIC: 0, CMD: 1, ARG0: 2, ARG1: 3, ARG2: 4, STATUS: 5, DONE: 6, PROGRESS: 7 };
const CMD = { IDLE: 0, ERASE: 1, PROGRAM: 2, EXIT: 3 };

/* ---- low-level TCF helpers ---- */
/* Memory.get reports a failed access in its error field and still returns a
 * (zero filled) buffer - never use that buffer: a backup of a sector that
 * could not be read would be all zeros, and restoring it would wipe the
 * sector.  Reads are split into 4 KiB requests. */
async function memRead(addr, size) {
    const parts = [];
    for (let off = 0; off < size; off += 0x1000) {
        const n = Math.min(0x1000, size - off);
        const a = (addr + off) >>> 0;
        const r = await cmdRaw('Memory', 'get', CORE, a, 1, n, 0);
        const err = r[1] && r[1] !== 'null' ? r[1] : null;
        if (err) throw new Error(`memory read failed at 0x${a.toString(16)} (+${n}): ${err.slice(0, 160)}`);
        const b = Buffer.from((r[0] || '').replace(/"/g, ''), 'base64');
        if (b.length !== n) throw new Error(`memory read at 0x${a.toString(16)}: got ${b.length} of ${n} bytes`);
        parts.push(b);
    }
    return Buffer.concat(parts);
}
async function memWrite(addr, data) {
    await cmdRaw('Memory', 'set', CORE, addr >>> 0, 1, data.length, 0, data.toString('base64'));
    const rb = await memRead(addr, data.length);
    if (!rb.equals(data)) throw new Error(`memWrite verify failed at 0x${(addr >>> 0).toString(16)}`);
}
async function regGet(idx) {
    const r = await cmd('Registers', 'get', `R${idx}.${CORE}`);
    return j(r) ? Buffer.from(j(r).replace(/"/g, ''), 'base64').readUInt32LE(0) : 0;
}
async function regSet(idx, val) {
    const b = Buffer.alloc(4); b.writeUInt32LE(val >>> 0, 0);
    await cmd('Registers', 'set', `R${idx}.${CORE}`, b.toString('base64'));
}
/* Read flash page by page (32 bytes).  An erased page of the TC27x PFLASH
 * does not read as 0: it fails with an uncorrectable ECC error (FLASH0_FSR
 * PFDBER), so a page that cannot be read is reported as blank.  Big reads
 * are tried first and split only where they fail. */
const PAGE = 32;
async function tryRead(addr, size) {
    const r = await cmdRaw('Memory', 'get', CORE, addr >>> 0, 1, size, 0);
    if (r[1] && r[1] !== 'null') return null;
    const b = Buffer.from((r[0] || '').replace(/"/g, ''), 'base64');
    return b.length === size ? b : null;
}
async function readPages(addr, size) {
    const image = Buffer.alloc(size);
    const blank = new Uint8Array(size / PAGE);           /* 1 = page unreadable (erased) */
    async function walk(a, n, step) {
        for (let off = 0; off < n; off += step) {
            const m = Math.min(step, n - off);
            const b = await tryRead(a + off, m);
            if (b) { b.copy(image, a + off - addr); continue; }
            if (m <= PAGE) { blank[(a + off - addr) / PAGE] = 1; continue; }
            await walk(a + off, m, m > 512 ? 512 : PAGE);
        }
    }
    await walk(addr, size, 0x1000);
    return { image, blank };
}
const countBlank = (blank) => blank.reduce((n, x) => n + x, 0);

async function mbRead(i) {
    const b = await memRead(MAILBOX + i * 4, 4);
    return b.readUInt32LE(0);
}
async function mbWrite(i, v) {
    const b = Buffer.alloc(4); b.writeUInt32LE(v >>> 0, 0);
    await memWrite(MAILBOX + i * 4, b);
}
async function waitMagic(timeoutMs) {
    const t0 = Date.now();
    for (;;) {
        const v = await mbRead(MB.MAGIC);
        if (v === MAGIC) return true;
        if (Date.now() - t0 > timeoutMs) return false;
        await sleep(200);
    }
}
async function runCmd(c, a0, a1, a2, timeoutMs = 120000) {
    if ((await mbRead(MB.MAGIC)) !== MAGIC) throw new Error('the flash loader is not running (no magic) - run "load" first');
    await mbWrite(MB.DONE, 0);
    await mbWrite(MB.PROGRESS, 0);
    await mbWrite(MB.ARG0, a0 >>> 0);
    await mbWrite(MB.ARG1, a1 >>> 0);
    await mbWrite(MB.ARG2, a2 >>> 0);
    /* no read-back of CMD: the loader takes the command and clears it at once */
    const cb = Buffer.alloc(4); cb.writeUInt32LE(c >>> 0, 0);
    await cmdRaw('Memory', 'set', CORE, (MAILBOX + MB.CMD * 4) >>> 0, 1, 4, 0, cb.toString('base64'));
    const t0 = Date.now();
    for (;;) {
        const done = await mbRead(MB.DONE);
        if (done === 1) {
            const status = await mbRead(MB.STATUS);
            const progress = await mbRead(MB.PROGRESS);
            return { status, progress };
        }
        if (Date.now() - t0 > timeoutMs) throw new Error(`command ${c} timeout (done=${done})`);
        await sleep(150);
    }
}

/* ---- commands ---- */
const REGIONS = [
    { base: 0x7000A000, size: 0x6000 },   /* DSPR payload + data + mailbox */
    { base: PSPR,       size: 0x2000 },   /* PSPR loader */
];

async function doSave() {
    console.log('=== saving state ===');
    const st0 = j(await cmd('RunControl', 'getState', CORE));
    const wasRunning = st0 === false || st0 === 'false' || st0 === 0;
    if (wasRunning) {
        console.log('core is running - suspending for a consistent snapshot');
        await cmd('RunControl', 'suspend', CORE);
        await sleep(300);
    }
    const regs = [];
    for (let i = 0; i < 40; i++) regs.push(j(await cmd('Registers', 'get', `R${i}.${CORE}`)));
    const regions = [];
    for (const r of REGIONS) {
        const mem = await memRead(r.base, r.size);
        regions.push({ base: r.base, size: r.size, b64: mem.toString('base64') });
        console.log(`  saved ${mem.length} bytes @0x${r.base.toString(16)}`);
    }
    /* remember core1/core2 run states (they must be halted while flashing) */
    const otherCores = [];
    for (const c of ['core1', 'core2']) {
        const st = j(await cmd('RunControl', 'getState', c));
        const running = (st === false || st === 'false' || st === 0);
        otherCores.push({ core: c, wasRunning: running });
    }
    fs.writeFileSync(STATE_FILE, JSON.stringify({ core: CORE, wasRunning, regs, regions, otherCores }));
    console.log(`saved 40 registers + ${regions.length} regions (wasRunning=${wasRunning}) -> ${STATE_FILE}`);
    process.exit(0);
}

async function doLoad() {
    if (!fs.existsSync(STATE_FILE)) throw new Error(`no saved state (${STATE_FILE}) - run "save" first`);
    const file = require('path').join(FLASHER_DIR, DEV.loader);
    const code = fs.readFileSync(file);
    console.log(`=== ${DEV.name}: uploading ${DEV.loader} (${code.length} bytes) to PSPR 0x${PSPR.toString(16)} ===`);
    await memWrite(PSPR, code);
    await regSet(await regIndex('PC'), PSPR);           /* PC = loader entry */
    await regSet(await regIndex('A10'), LOADER_SP);     /* SP */
    /* no application interrupt may run while a flash bank is busy */
    const icr = await regIndex('ICR');
    await regSet(icr, (await regGet(icr)) & ~(1 << 15)); /* ICR.IE = 0 */
    /* halt core1/core2 as well: the application may reset the device when
     * core0 stops responding (cross-core monitor) */
    for (const c of ['core1', 'core2']) {
        try {
            await cmd('RunControl', 'suspend', c);
            console.log(`  ${c} suspended`);
        } catch (e) {
            console.log(`  ${c} suspend: ${e.message}`);
        }
    }
    const st = j(await cmd('RunControl', 'getState', CORE));
    console.log('core state before resume:', st);
    await cmd('RunControl', 'resume', CORE, 0, 1);
    const ok = await waitMagic(8000);
    console.log(ok ? 'loader ready (magic OK)' : 'MAGIC NOT FOUND - loader did not start');
    process.exit(ok ? 0 : 1);
}

/* erase must start at a logical sector and stay inside PFLASH */
function checkEraseRange(addr, count) {
    const first = sectorAt(addr);
    if (!first || first.base !== addr) throw new Error(`0x${addr.toString(16)} is not the start of a ${DEV.name} PFLASH sector`);
    const i = DEV.sectors.indexOf(first);
    if (count < 1 || i + count > DEV.sectors.length) throw new Error('sector count out of range');
    return DEV.sectors.slice(i, i + count);
}

async function doErase(addr, count) {
    const list = checkEraseRange(addr, count);
    const bytes = list.reduce((n, s) => n + s.size, 0);
    console.log(`=== erase ${count} sector(s) @0x${addr.toString(16)} (${bytes / 1024} KiB) ===`);
    const r = await runCmd(CMD.ERASE, addr, count, 0);
    console.log(`erase done: status=0x${r.status.toString(16)} progress=${r.progress}/${count}`);
    process.exit(r.status === 0 ? 0 : 1);
}

async function doProgram(addr, file) {
    const data = fs.readFileSync(file);
    if (!sectorAt(addr) || !sectorAt(addr + data.length - 1) || (addr & 31))
        throw new Error(`0x${addr.toString(16)}+${data.length} is not a 32-byte aligned ${DEV.name} PFLASH range`);
    console.log(`=== program ${data.length} bytes from ${file} at 0x${addr.toString(16)} ===`);
    const CHUNK = 0x2000;   /* 8 KiB staging buffer */
    let off = 0;
    while (off < data.length) {
        const n = Math.min(CHUNK, data.length - off);
        const chunk = data.subarray(off, off + n);
        const padded = Buffer.alloc((n + 31) & ~31);
        chunk.copy(padded);
        await memWrite(PAYLOAD, padded);
        const r = await runCmd(CMD.PROGRAM, addr + off, padded.length, PAYLOAD);
        console.log(`  @0x${(addr + off).toString(16)}: status=0x${r.status.toString(16)} pages=${r.progress}/${padded.length / 32}`);
        if (r.status !== 0) { console.log('PROGRAM FAILED'); process.exit(1); }
        off += n;
    }
    console.log('program done');
    process.exit(0);
}

async function doRead(addr, size, out) {
    const mem = await memRead(addr, size);
    fs.writeFileSync(out, mem);
    console.log(`read ${mem.length} bytes @0x${addr.toString(16)} -> ${out}`);
    process.exit(0);
}

async function doVerify(addr, file) {
    const data = fs.readFileSync(file);
    const mem = await memRead(addr, data.length);
    if (mem.equals(data)) {
        console.log(`VERIFY OK (${data.length} bytes @0x${addr.toString(16)})`);
        process.exit(0);
    }
    let bad = -1;
    for (let i = 0; i < data.length; i++) if (mem[i] !== data[i]) { bad = i; break; }
    console.log(`VERIFY MISMATCH at offset 0x${bad.toString(16)} (addr 0x${(addr + bad).toString(16)}): flash=0x${mem[bad].toString(16)} file=0x${data[bad].toString(16)}`);
    process.exit(1);
}

/* ---- page aware sector backup / program / verify ----
 * A sector image is { base, image, blank }: blank[i] = 1 when page i is
 * erased (unreadable).  Only programmed pages are written back; erased pages
 * stay erased, so a restored sector reads exactly like the original. */
function saveBackup(file, base, img) {
    fs.writeFileSync(file, JSON.stringify({ device: DEV.name, base, size: img.image.length,
        blank: Array.from(img.blank), b64: img.image.toString('base64') }));
}
function loadBackup(file) {
    const o = JSON.parse(fs.readFileSync(file, 'utf8'));
    if (o.device !== DEV.name) throw new Error(`${file} is a ${o.device} backup, the target is ${DEV.name}`);
    const sec = sectorAt(o.base);
    if (!sec || sec.base !== o.base || sec.size !== o.size) throw new Error(`${file} does not match a ${DEV.name} sector`);
    return { base: o.base, image: Buffer.from(o.b64, 'base64'), blank: Uint8Array.from(o.blank) };
}
async function programPages(base, image, blank) {
    let written = 0;
    for (let p = 0; p < blank.length;) {
        if (blank[p]) { p++; continue; }
        let q = p;
        while (q < blank.length && !blank[q] && (q - p) * PAGE < 0x2000) q++;
        const chunk = image.subarray(p * PAGE, q * PAGE);
        await memWrite(PAYLOAD, chunk);
        const r = await runCmd(CMD.PROGRAM, base + p * PAGE, chunk.length, PAYLOAD);
        if (r.status !== 0) throw new Error(`program @0x${(base + p * PAGE).toString(16)} failed (status 0x${r.status.toString(16)})`);
        written += q - p;
        p = q;
    }
    return written;
}
async function verifyPages(base, image, blank) {
    const rb = await readPages(base, image.length);
    for (let i = 0; i < blank.length; i++) {
        const at = `0x${(base + i * PAGE).toString(16)}`;
        if (blank[i] !== rb.blank[i]) return `${at}: expected ${blank[i] ? 'erased' : 'data'}, found ${rb.blank[i] ? 'erased' : 'data'}`;
        if (!blank[i] && !image.subarray(i * PAGE, (i + 1) * PAGE).equals(rb.image.subarray(i * PAGE, (i + 1) * PAGE)))
            return `${at}: data differs`;
    }
    return null;
}
async function eraseSector(sec) {
    const r = await runCmd(CMD.ERASE, sec.base, 1, 0);
    if (r.status !== 0) throw new Error(`erase 0x${sec.base.toString(16)} failed (status 0x${r.status.toString(16)})`);
}
/* erase a sector and write a backup back, then verify */
async function restoreSector(bk) {
    const sec = sectorAt(bk.base);
    await eraseSector(sec);
    const n = await programPages(bk.base, bk.image, bk.blank);
    const bad = await verifyPages(bk.base, bk.image, bk.blank);
    if (bad) throw new Error(`restore verify failed: ${bad}`);
    console.log(`0x${bk.base.toString(16)}: erased, ${n} page(s) written, verified (${countBlank(bk.blank)} erased page(s))`);
}

async function doBackup(addr, file) {
    const sec = sectorAt(toUncached(addr));
    if (!sec || sec.base !== toUncached(addr)) throw new Error(`0x${addr.toString(16)} is not a sector start`);
    const img = await readPages(sec.base, sec.size);
    saveBackup(file, sec.base, img);
    console.log(`backup 0x${sec.base.toString(16)} (${sec.size / 1024} KiB): ${img.blank.length - countBlank(img.blank)} programmed / ${countBlank(img.blank)} erased page(s) -> ${file}`);
    process.exit(0);
}

async function doVerifySector(file) {
    const bk = loadBackup(file);
    const bad = await verifyPages(bk.base, bk.image, bk.blank);
    console.log(bad ? `SECTOR DIFFERS from ${file}: ${bad}` : `sector 0x${bk.base.toString(16)} matches ${file}`);
    process.exit(bad ? 1 : 0);
}

/* restore <backup.json> - erase the sector and write the backup back */
async function doRestore(file) {
    console.log(`=== restore ${file} ===`);
    await restoreSector(loadBackup(file));
    process.exit(0);
}
async function doStop() {
    console.log('=== stopping loader ===');
    try {
        const r = await runCmd(CMD.EXIT, 0, 0, 0, 10000);
        console.log(`loader exit: status=0x${r.status.toString(16)}`);
    } catch (e) {
        console.log('loader exit skipped:', e.message);
    }
    await cmd('RunControl', 'suspend', CORE);
    if (fs.existsSync(STATE_FILE)) {
        const st = JSON.parse(fs.readFileSync(STATE_FILE, 'utf8'));
        console.log('restoring registers + scratch regions');
        for (let i = 0; i < st.regs.length; i++) {
            if (st.regs[i]) await cmd('Registers', 'set', `R${i}.${CORE}`, st.regs[i]);
        }
        for (const r of st.regions) {
            const b = Buffer.from(r.b64, 'base64');
            await memWrite(r.base, b);
            console.log(`  restored ${b.length} bytes @0x${r.base.toString(16)}`);
        }
        console.log('state restored');
        if (st.wasRunning) {
            console.log('core was running before - resuming');
            await cmd('RunControl', 'resume', CORE, 0, 1);
        }
        if (Array.isArray(st.otherCores)) {
            for (const oc of st.otherCores) {
                if (oc.wasRunning) {
                    try {
                        await cmd('RunControl', 'resume', oc.core, 0, 1);
                        console.log(`${oc.core} resumed`);
                    } catch (e) {
                        console.log(`${oc.core} resume: ${e.message}`);
                    }
                }
            }
        }
    } else {
        console.log('no state file - registers not restored');
    }
    process.exit(0);
}

/* read whole sectors and report which hold data (read only) */
async function doScan(start, count) {
    let s = sectorAt(toUncached(start));
    if (!s) throw new Error(`0x${start.toString(16)} is not in ${DEV.name} PFLASH`);
    console.log(`=== ${DEV.name}: scanning ${count} sector(s) from 0x${s.base.toString(16)} ===`);
    for (let i = DEV.sectors.indexOf(s), n = 0; n < count && DEV.sectors[i]; i++, n++) {
        s = DEV.sectors[i];
        const { blank } = await readPages(s.base, s.size);
        const pages = blank.length, nb = countBlank(blank);
        console.log(`0x${s.base.toString(16)} (${s.size / 1024} KiB): ` +
            (nb === pages ? 'blank (all pages erased)' : `${pages - nb} programmed page(s), ${nb} blank`));
    }
    process.exit(0);
}

async function doBp() {
    const ids = j(await cmd('Breakpoints', 'getIDs'));
    console.log('breakpoint IDs:', JSON.stringify(ids));
    if (Array.isArray(ids) && ids.length) {
        const props = j(await cmd('Breakpoints', 'getProperties', ids));
        console.log('properties:', JSON.stringify(props));
        const st = await cmd('Breakpoints', 'getStatus');
        console.log('status:', JSON.stringify(st));
    }
    process.exit(0);
}

async function doBpClear() {
    const ids = j(await cmd('Breakpoints', 'getIDs'));
    console.log('breakpoint IDs:', JSON.stringify(ids));
    if (Array.isArray(ids) && ids.length) {
        for (const id of ids) {
            try {
                await cmd('Breakpoints', 'remove', id);
                console.log(`removed ${id}`);
            } catch (e) {
                console.log(`remove ${id} failed: ${e.message}`);
            }
        }
    } else {
        console.log('no breakpoints registered');
    }
    process.exit(0);
}

/* ---- ELF32 (little-endian, TriCore) program header parser ---- */
function parseElf(buf) {
    if (buf.length < 52 || buf[0] !== 0x7F || buf[1] !== 0x45 || buf[2] !== 0x4C || buf[3] !== 0x46) {
        throw new Error('not an ELF file');
    }
    if (buf[4] !== 1 || buf[5] !== 1) throw new Error('expected ELF32 little-endian');
    const phoff = buf.readUInt32LE(0x1C);
    const phentsize = buf.readUInt16LE(0x2A);
    const phnum = buf.readUInt16LE(0x2C);
    const segs = [];
    for (let i = 0; i < phnum; i++) {
        const o = phoff + i * phentsize;
        if (buf.readUInt32LE(o) !== 1) continue;          /* PT_LOAD */
        const offset = buf.readUInt32LE(o + 4);
        const paddr = buf.readUInt32LE(o + 12);
        const filesz = buf.readUInt32LE(o + 16);
        if (filesz === 0) continue;
        segs.push({ paddr, filesz, data: buf.subarray(offset, offset + filesz) });
    }
    return segs;
}
/* cached 0x8xxxxxxx -> uncached 0xAxxxxxxx (the flash command interface uses these) */
const toUncached = (a) => ((((a >>> 0) >>> 28) === 0x8) ? ((a >>> 0) + 0x20000000) >>> 0 : (a >>> 0));
const inPflash = (a) => sectorAt(toUncached(a)) !== undefined;

async function doFlash(elfPath, opts) {
    const buf = fs.readFileSync(elfPath);
    const segs = parseElf(buf).filter((s) => inPflash(s.paddr));
    if (segs.length === 0) {
        console.log('no PFLASH segments in this ELF');
        process.exit(1);
    }
    console.log(`=== flash ${elfPath} ===`);
    for (const s of segs) {
        console.log(`  PT_LOAD: 0x${s.paddr.toString(16)}..0x${(s.paddr + s.filesz - 1).toString(16)} (${s.filesz} bytes)`);
    }
    /* group ELF bytes per logical sector of the device (uncached addresses) */
    const sectors = new Map();
    for (const s of segs) {
        for (let off = 0; off < s.filesz; off++) {
            const addr = toUncached(s.paddr + off);
            const sec = sectorAt(addr);
            if (!sec) throw new Error(`0x${(s.paddr + off).toString(16)} is outside ${DEV.name} PFLASH`);
            let e = sectors.get(sec.base);
            if (!e) { e = { base: sec.base, size: sec.size, patches: [] }; sectors.set(sec.base, e); }
            e.patches.push({ addr, byte: s.data[off] });
        }
    }
    const list = [...sectors.values()].sort((a, b) => a.base - b.base);
    console.log(`sectors to touch: ${list.length}`);
    let flashed = 0, skipped = 0;
    for (const e of list) {
        const cur = await readPages(e.base, e.size);
        const image = Buffer.from(cur.image);
        const blank = Uint8Array.from(cur.blank);
        for (const p of e.patches) {
            const off = p.addr - e.base;
            const pg = Math.floor(off / PAGE);
            if (blank[pg]) { image.fill(0, pg * PAGE, (pg + 1) * PAGE); blank[pg] = 0; }   /* new page */
            image[off] = p.byte;
        }
        let same = true;
        for (let i = 0; i < blank.length && same; i++) {
            if (blank[i] !== cur.blank[i]) same = false;
            else if (!blank[i] && !image.subarray(i * PAGE, (i + 1) * PAGE).equals(cur.image.subarray(i * PAGE, (i + 1) * PAGE))) same = false;
        }
        if (same) {
            console.log(`0x${e.base.toString(16)}: already matches - skip`);
            skipped++;
            continue;
        }
        /* backup before touching */
        const bak = `${elfPath}.sector_${e.base.toString(16)}.json`;
        saveBackup(bak, e.base, cur);
        console.log(`0x${e.base.toString(16)} (${e.size / 1024} KiB): backup -> ${bak}`);
        if (opts.dryRun) { console.log('  (dry-run: not writing)'); continue; }
        await eraseSector(sectorAt(e.base));
        const n = await programPages(e.base, image, blank);
        const bad = await verifyPages(e.base, image, blank);
        if (bad) { console.log(`  VERIFY FAILED: ${bad}`); process.exit(1); }
        console.log(`0x${e.base.toString(16)}: erased, ${n} page(s) programmed, verified`);
        flashed++;
    }
    console.log(`flash done: ${flashed} sector(s) written, ${skipped} skipped`);
    process.exit(0);
}

async function doFlashRestore(elfPath) {
    /* restore all sector backups taken by doFlash */
    const dir = require('path').dirname(elfPath);
    const prefix = require('path').basename(elfPath) + '.sector_';
    const files = fs.readdirSync(dir).filter((f) => f.startsWith(prefix) && f.endsWith('.json'));
    if (files.length === 0) { console.log('no sector backups found'); process.exit(1); }
    for (const f of files.sort()) await restoreSector(loadBackup(require('path').join(dir, f)));
    console.log('restore done');
    process.exit(0);
}

async function doResume() {
    const st = j(await cmd('RunControl', 'getState', CORE));
    console.log('state before resume:', JSON.stringify(st));
    const r = await cmd('RunControl', 'resume', CORE, 0, 1);
    console.log('resume ->', JSON.stringify(r));
    await sleep(500);
    const st2 = j(await cmd('RunControl', 'getState', CORE));
    console.log('state after resume:', JSON.stringify(st2));
    process.exit(0);
}

async function doW32(addr, val) {
    const b = Buffer.alloc(4); b.writeUInt32LE(val >>> 0, 0);
    await memWrite(addr >>> 0, b);
    console.log(`wrote 0x${(val >>> 0).toString(16)} to 0x${(addr >>> 0).toString(16)}`);
    process.exit(0);
}

async function doReset(suspend) {
    const r = await cmdRaw('ContextReset', 'reset', CORE, 'reset', { Suspend: suspend });
    console.log(`reset (suspend=${suspend}):`, JSON.stringify(r.slice(0, 3)));
    await sleep(1500);
    process.exit(0);
}

async function doReg(idx, val) {
    if (val === undefined) {
        const v = await regGet(idx);
        console.log(`R${idx} = 0x${(v >>> 0).toString(16)}`);
    } else {
        await regSet(idx, val);
        const v = await regGet(idx);
        console.log(`R${idx} <- 0x${(val >>> 0).toString(16)} (readback 0x${(v >>> 0).toString(16)})`);
    }
    process.exit(0);
}

async function doBpSet(addr) {
    const r = await cmdRaw('Breakpoints', 'set', [{
        Location: '0x' + (addr >>> 0).toString(16),
        AccessMode: 4,
        ContextIds: [CORE],
    }]);
    console.log(`bpset 0x${(addr >>> 0).toString(16)}:`, JSON.stringify(r.slice(0, 2)));
    process.exit(0);
}

async function doStep(mode) {
    const r = await cmdRaw('RunControl', 'resume', CORE, mode, 1);
    console.log(`step mode=${mode}:`, JSON.stringify(r.slice(0, 2)));
    await sleep(500);
    const v = await memRead(0xF881FE08, 4);
    console.log(`CSFR PC after step = 0x${v.readUInt32LE(0).toString(16)}`);
    process.exit(0);
}

async function doHalt() {
    try {
        await cmd('RunControl', 'suspend', CORE);
        console.log('halt issued');
    } catch (e) {
        console.log('halt:', e.message);
    }
    await sleep(500);
    const v = await memRead(0xF881FE08, 4);
    console.log(`CSFR PC after halt = 0x${v.readUInt32LE(0).toString(16)}`);
    process.exit(0);
}

(async () => {
    await sleep(300);
    const c = (process.argv[2] || '').toLowerCase();
    const num = (x, d) => (x !== undefined ? Number(x) : d);
    try {
        /* every flash command depends on the device profile */
        if (c === 'sectors') {          /* print a profile's sector map (no target access) */
            const d = DEVICES[(process.argv[3] || 'tc27x').toLowerCase()];
            console.log(JSON.stringify(d.sectors.map((s) => [s.base, s.size])));
            process.exit(0);
        }
        if (['device', 'load', 'erase', 'program', 'restore', 'backup', 'verifysector', 'flash', 'flashrestore', 'scan'].includes(c)) {
            const id = await detectDevice();
            console.log(`device: ${DEV.name} (JTAG ID 0x${id.toString(16)}, ${DEV.sectors.length} PFLASH sectors, loader ${DEV.loader})`);
            if (c === 'device') process.exit(0);
        }
        if (c === 'save') await doSave();
        else if (c === 'load') await doLoad();
        else if (c === 'erase') await doErase(num(process.argv[3], 0xA0000000) >>> 0, num(process.argv[4], 1));
        else if (c === 'program') await doProgram(num(process.argv[3], 0) >>> 0, process.argv[4]);
        else if (c === 'read') await doRead(num(process.argv[3], 0) >>> 0, num(process.argv[4], 0x1000), process.argv[5] || 'read.bin');
        else if (c === 'verify') await doVerify(num(process.argv[3], 0) >>> 0, process.argv[4]);
        else if (c === 'restore') await doRestore(process.argv[3]);
        else if (c === 'backup') await doBackup(num(process.argv[3], 0) >>> 0, process.argv[4]);
        else if (c === 'verifysector') await doVerifySector(process.argv[3]);
        else if (c === 'stop') await doStop();
        else if (c === 'bpset') await doBpSet(num(process.argv[3],0) >>> 0);
        else if (c === 'bp') await doBp();
        else if (c === 'bpclear') await doBpClear();
        else if (c === 'reg') await doReg(num(process.argv[3],0), process.argv[4] !== undefined ? num(process.argv[4],0) : undefined);
        else if (c === 'reset') await doReset(process.argv[3] !== 'run');
        else if (c === 'halt') await doHalt();
        else if (c === 'step') await doStep(num(process.argv[3],4));
        else if (c === 'resume') await doResume();
        else if (c === 'w32') await doW32(num(process.argv[3],0), num(process.argv[4],0));
        else if (c === 'flash') await doFlash(process.argv[3], { dryRun: process.argv.includes('--dry-run') });
        else if (c === 'flashrestore') await doFlashRestore(process.argv[3]);
        else if (c === 'scan') await doScan(num(process.argv[3], 0x80000000) >>> 0, num(process.argv[4], 8));
        else {
            console.log('usage: device | sectors [tc27x|tc37x] | scan <addr> [n] | backup <sector> <file.json> | save | load | erase <sector> [count] | program <addr> <file> | read <addr> <size> <out> | verify <addr> <file> | verifysector <file.json> | restore <file.json> | flash <elf> [--dry-run] | flashrestore <elf> | stop');
            process.exit(1);
        }
    } catch (e) {
        console.error('FAILED:', e.message);
        process.exit(1);
    }
})();
