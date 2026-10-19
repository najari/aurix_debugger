// Restore the state saved by ram_load.js: the RAM windows and all core0
// registers, so the flashed application continues as if nothing happened.
//
// usage: node tools/ram_restore.js
'use strict';
const fs = require('fs');
const path = require('path');
const { connect } = require('./tcf');

(async () => {
    const root = path.resolve(__dirname, '..');
    const stateFile = path.join(root, 'state', 'ram_state.json');
    const s = JSON.parse(fs.readFileSync(stateFile, 'utf8'));
    const c = await connect();

    let st = await c.state(s.core);
    if (!st.suspended) { await c.suspend(s.core); await new Promise(r => setTimeout(r, 400)); st = await c.state(s.core); }
    if (!st.suspended) throw new Error(`${s.core} is not suspended`);

    // Every CALL takes a CSA off the free list and overwrites its link word;
    // only the matching RET puts it back.  Restoring the registers while demo
    // calls are still open would leave the application with a broken free
    // list (FCU trap at its next call), so the call depth must be 0 - the
    // free list head (FCX) is then back where it was when the state was saved.
    const { map } = await c.regIds(s.core);
    const fcx = await c.getReg(map.FCX);
    if (fcx !== (s.regs[map.FCX] >>> 0)) {
        throw new Error(`demo calls are still open (FCX 0x${fcx.toString(16)}, saved 0x${(s.regs[map.FCX] >>> 0).toString(16)}): ` +
            'stop core0 in main first (breakpoint at the "j" after "call compute", 0x701000b0), then run this again');
    }

    for (const m of s.mem) {
        const data = Buffer.from(m.b64, 'base64');
        await c.writeMem(s.core, m.addr, data);
        const rb = await c.readMem(s.core, m.addr, data.length);
        console.log(`RAM @0x${m.addr.toString(16)} (${data.length} B): ${rb.equals(data) ? 'MATCH' : 'MISMATCH'}`);
    }
    let fails = 0;
    for (const [id, v] of Object.entries(s.regs)) {
        try { await c.setReg(id, v); } catch (e) { fails++; console.log(`  ${e.message}`); }
    }
    let bad = 0;
    for (const [id, v] of Object.entries(s.regs)) if ((await c.getReg(id)) !== (v >>> 0)) bad++;
    console.log(`registers restored: ${Object.keys(s.regs).length - fails}/${Object.keys(s.regs).length}, readback mismatches: ${bad}`);
    st = await c.state(s.core);
    console.log(`core0 suspended=${st.suspended} pc=0x${st.pc.toString(16)} (resume it to continue the application)`);
    if (!fails && !bad) {
        // used up: the next ram_load.js saves a fresh application state
        const done = stateFile.replace(/\.json$/, `.restored-${Date.now()}.json`);
        fs.renameSync(stateFile, done);
        console.log(`state file moved to ${path.basename(done)}`);
    }
    c.close();
    process.exit(fails || bad ? 1 : 0);
})().catch((e) => { console.error('FAILED:', e.message); process.exit(1); });
