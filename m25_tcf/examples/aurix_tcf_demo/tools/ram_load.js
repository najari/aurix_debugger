// Load build/demo.elf into the CPU0 RAM (PSPR code, DSPR data) and point the
// core at main.  The core state (all registers) and the RAM windows used by
// the demo are saved first to state/ram_state.json; ram_restore.js puts the
// flashed application back exactly as it was.
//
// usage: node tools/ram_load.js [elf]
'use strict';
const fs = require('fs');
const path = require('path');
const { connect } = require('./tcf');

const CORE = 'core0';
const SAVE = [
    { addr: 0x70100000, size: 0x2000 },   // PSPR window (code)
    { addr: 0x7000A000, size: 0x2000 },   // DSPR window (data + stack)
];
const STACK_TOP = 0x7000C000;

// Load by ALLOC sections, not by PT_LOAD segments: the linker may map the
// ELF/program headers into a segment in front of the first section
// (observed: a data segment at 0x70009F88 for .bss at 0x7000A000), and
// writing that would overwrite target memory outside the program.
function loadSegments(elfPath) {
    const b = fs.readFileSync(elfPath);
    if (b.readUInt32BE(0) !== 0x7f454c46 || b[4] !== 1 || b[5] !== 1) throw new Error('not an ELF32 LE file');
    const entry = b.readUInt32LE(24);
    const shoff = b.readUInt32LE(32), shent = b.readUInt16LE(46), shnum = b.readUInt16LE(48);
    const shstrndx = b.readUInt16LE(50);
    const strOff = b.readUInt32LE(shoff + shstrndx * shent + 16);
    const segs = [];
    for (let i = 0; i < shnum; i++) {
        const o = shoff + i * shent;
        const type = b.readUInt32LE(o + 4), flags = b.readUInt32LE(o + 8);
        const addr = b.readUInt32LE(o + 12), off = b.readUInt32LE(o + 16), size = b.readUInt32LE(o + 20);
        if ((flags & 0x2) === 0 || size === 0) continue;        // SHF_ALLOC only
        let name = '';
        for (let k = strOff + b.readUInt32LE(o); b[k]; k++) name += String.fromCharCode(b[k]);
        const data = Buffer.alloc(size);                        // SHT_NOBITS (.bss) stays 0
        if (type !== 8) b.copy(data, 0, off, off + size);
        segs.push({ vaddr: addr, data, name });
    }
    return { entry, segs };
}

(async () => {
    const root = path.resolve(__dirname, '..');
    const elf = process.argv[2] || path.join(root, 'build', 'demo.elf');
    const { entry, segs } = loadSegments(elf);
    for (const s of segs) {
        const ok = SAVE.some(w => s.vaddr >= w.addr && s.vaddr + s.data.length <= w.addr + w.size);
        if (!ok) throw new Error(`segment 0x${s.vaddr.toString(16)} is outside the saved RAM windows`);
    }

    // Never overwrite a saved application state with the demo's own state.
    const stateFile = path.join(root, 'state', 'ram_state.json');
    if (fs.existsSync(stateFile)) {
        throw new Error(`${stateFile} already exists - the demo is loaded; run tools/ram_restore.js first`);
    }

    const c = await connect();
    let st = await c.state(CORE);
    if (!st.suspended) { await c.suspend(CORE); await new Promise(r => setTimeout(r, 400)); st = await c.state(CORE); }
    if (!st.suspended) throw new Error(`${CORE} is not suspended`);

    // save
    const { ids, map } = await c.regIds(CORE);
    const regs = {};
    for (const id of ids) regs[id] = await c.getReg(id);
    const mem = [];
    for (const w of SAVE) mem.push({ addr: w.addr, b64: (await c.readMem(CORE, w.addr, w.size)).toString('base64') });
    fs.mkdirSync(path.join(root, 'state'), { recursive: true });
    fs.writeFileSync(stateFile, JSON.stringify({ core: CORE, regs, mem, saved: new Date().toISOString() }));
    console.log(`saved ${ids.length} registers + ${SAVE.length} RAM windows -> ${stateFile}`);
    console.log(`  application PC was 0x${regs[map.PC].toString(16)}`);

    // load + verify
    for (const s of segs) {
        await c.writeMem(CORE, s.vaddr, s.data);
        const rb = await c.readMem(CORE, s.vaddr, s.data.length);
        console.log(`loaded ${s.name} ${s.data.length} B @0x${s.vaddr.toString(16)}: ${rb.equals(s.data) ? 'MATCH' : 'MISMATCH'}`);
        if (!rb.equals(s.data)) throw new Error('readback mismatch');
    }

    // core: PC = entry, A10 = stack, interrupts off (ICR.IE = bit 15).
    // A11 = 0 and PCXI = 0 make main the outermost frame (as after reset);
    // otherwise the call stack continues into the application's contexts.
    await c.setReg(map.PC, entry);
    await c.setReg(map.A10, STACK_TOP);
    await c.setReg(map.A11, 0);
    await c.setReg(map.PCXI, 0);
    const icr = await c.getReg(map.ICR);
    await c.setReg(map.ICR, icr & ~(1 << 15));
    console.log(`PC=0x${entry.toString(16)} (main)  A10=0x${STACK_TOP.toString(16)}  A11=0 PCXI=0  ICR 0x${icr.toString(16)} -> 0x${(icr & ~(1 << 15)).toString(16)}`);
    console.log('ready: core0 is suspended at main - set breakpoints and continue');
    c.close();
    process.exit(0);
})().catch((e) => { console.error('FAILED:', e.message); process.exit(1); });
