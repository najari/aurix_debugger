// Point core0 at main of the flashed demo (demo_flash.elf, PF1 0x8021C000).
// The flasher leaves the application running after "reset run"; this suspends
// core0, sets PC = ELF entry, A10 = stack, A11 = 0, PCXI = 0 (main becomes the
// outermost frame) and clears ICR.IE.  Core0 stays suspended at main.
// Run it while the debug session is up (the agent listens on 127.0.0.1:1534).
// The application's context is abandoned: after the session, reset the board
// (the application in flash is untouched) - nothing is saved or restored here.
//
// usage: node tools/flash_start.js [elf]
'use strict';
const fs = require('fs');
const path = require('path');
const { connect } = require('./tcf');

const CORE = 'core0';
const STACK_TOP = 0x7000C000;

(async () => {
    const elf = process.argv[2] || path.join(__dirname, '..', 'build', 'demo_flash.elf');
    const b = fs.readFileSync(elf);
    if (b.readUInt32BE(0) !== 0x7f454c46 || b[4] !== 1 || b[5] !== 1) throw new Error('not an ELF32 LE file');
    const entry = b.readUInt32LE(24);

    const c = await connect();
    let st = await c.state(CORE);
    if (!st.suspended) { await c.suspend(CORE); await new Promise(r => setTimeout(r, 400)); st = await c.state(CORE); }
    if (!st.suspended) throw new Error(`${CORE} is not suspended`);

    // The code in flash must be the demo (a stale image would run garbage).
    const phoff = b.readUInt32LE(28), phnum = b.readUInt16LE(44), phent = b.readUInt16LE(42);
    for (let i = 0; i < phnum; i++) {
        const o = phoff + i * phent;
        if (b.readUInt32LE(o) !== 1) continue;                      // PT_LOAD
        const off = b.readUInt32LE(o + 4), vaddr = b.readUInt32LE(o + 8), fsz = b.readUInt32LE(o + 16);
        const rb = await c.readMem(CORE, vaddr, fsz);
        if (!rb.equals(b.subarray(off, off + fsz))) {
            throw new Error(`flash at 0x${vaddr.toString(16)} does not match ${path.basename(elf)} - flash it first (F5 with the flash task)`);
        }
        console.log(`flash @0x${vaddr.toString(16)} ${fsz} B: MATCH`);
    }

    const { map } = await c.regIds(CORE);
    await c.setReg(map.PC, entry);
    await c.setReg(map.A10, STACK_TOP);
    await c.setReg(map.A11, 0);
    await c.setReg(map.PCXI, 0);
    const icr = await c.getReg(map.ICR);
    await c.setReg(map.ICR, icr & ~(1 << 15));
    console.log(`PC=0x${entry.toString(16)} (main)  A10=0x${STACK_TOP.toString(16)}  A11=0 PCXI=0  ICR.IE off`);
    console.log('ready: core0 is suspended at main - set breakpoints and continue');
    c.close();
    process.exit(0);
})().catch((e) => { console.error('FAILED:', e.message); process.exit(1); });
