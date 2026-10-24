// One DAP session for the three cores of the TC275 (PRD "한 세션 멀티코어", tests T-M1..T-M10).
// CPU0 must be halted at main of demo_flash.elf (start_in_flash); CPU1 and CPU2 run the PF0
// application. Nothing is written to flash.
//
//   node dap_onesession.js            allStop: true (T-M1..T-M6, T-M9, T-M10)
//   node dap_onesession.js noallstop  allStop: false (T-M7)
'use strict';
const { spawn, execFileSync } = require('child_process');
const DEMO = 'C:\\work\\aurix_tcf_demo';
const ELF = `${DEMO}\\build\\demo_flash.elf`;
const SRC = `${DEMO}\\src\\demo.c`;
const PROBE_RS = 'C:\\work\\probe-rs\\target\\debug\\probe-rs.exe';
const results = [];
const check = (what, ok, detail) => { results.push(ok); console.log(`[${ok ? 'PASS' : 'FAIL'}] ${what}: ${detail}`); };
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const hex = (v) => `0x${Number(v).toString(16)}`;
const t0 = Date.now();
const now = () => `${String(Date.now() - t0).padStart(6)} ms`;

class Dap {
    constructor() {
        this.p = spawn(PROBE_RS, ['dap-server'], { stdio: ['pipe', 'pipe', 'inherit'] });
        this.seq = 1; this.buf = Buffer.alloc(0); this.pending = new Map(); this.waiters = []; this.log = []; this.out = [];
        this.p.stdout.on('data', (c) => this.onData(c));
    }
    onData(c) {
        this.buf = Buffer.concat([this.buf, c]);
        for (;;) {
            const sep = this.buf.indexOf('\r\n\r\n'); if (sep < 0) return;
            const len = Number(/Content-Length: (\d+)/.exec(this.buf.slice(0, sep).toString())[1]);
            if (this.buf.length < sep + 4 + len) return;
            const m = JSON.parse(this.buf.slice(sep + 4, sep + 4 + len).toString()); this.buf = this.buf.slice(sep + 4 + len);
            if (m.type === 'response') { const r = this.pending.get(m.request_seq); this.pending.delete(m.request_seq); if (r) r(m); continue; }
            if (m.type !== 'event') continue;
            if (m.event === 'output') {
                this.out.push(m.body.output);
                if (/rror|All-stop/.test(m.body.output)) console.log(`  [output] ${m.body.output.trim().slice(0, 200)}`);
            } else if (m.event === 'stopped' || m.event === 'continued') {
                this.log.push({ event: m.event, thread: m.body.threadId, reason: m.body.reason, body: m.body, at: Date.now() });
                console.log(`  ${now()} thread ${m.body.threadId} ${m.event}${m.body.reason ? ` (${m.body.reason})` : ''}`);
            }
            const i = this.waiters.findIndex((w) => w.name === m.event && (w.thread === undefined || w.thread === m.body.threadId));
            if (i >= 0) this.waiters.splice(i, 1)[0].resolve(m.body);
        }
    }
    send(command, args) {
        return new Promise((r) => { const msg = { seq: this.seq++, type: 'request', command, arguments: args }; const b = Buffer.from(JSON.stringify(msg)); this.p.stdin.write(`Content-Length: ${b.length}\r\n\r\n`); this.p.stdin.write(b); this.pending.set(msg.seq, r); });
    }
    event(name, thread, ms) {
        return new Promise((resolve) => { const w = { name, thread, resolve: (e) => { clearTimeout(t); resolve(e); } }; const t = setTimeout(() => { const i = this.waiters.indexOf(w); if (i >= 0) this.waiters.splice(i, 1); resolve(null); }, ms); this.waiters.push(w); });
    }
    async attach(allStop, extra = {}) {
        this.capabilities = (await this.send('initialize', { adapterID: 'probe-rs-debug', linesStartAt1: true, columnsStartAt1: true, pathFormat: 'path' })).body;
        const initialized = this.event('initialized', undefined, 60000);
        const r = await this.send('attach', {
            cwd: DEMO, chip: 'TC275', probe: '058b:0000', allStop,
            coreConfigs: [0, 1, 2].map((coreIndex) => ({ coreIndex, programBinary: ELF, ...(extra[coreIndex] || {}) })),
            consoleLogLevel: 'Console',
        });
        if (!r.success) throw new Error(`attach: ${r.message} ${JSON.stringify(r.body)}`);
        await initialized;
        const c = await this.send('configurationDone', {});
        if (!c.success) throw new Error(`configurationDone: ${JSON.stringify(c.body)}`);
        return (await this.send('threads', {})).body.threads;
    }
    async top(thread) {
        const r = await this.send('stackTrace', { threadId: thread, startFrame: 0, levels: 1 });
        if (!r.success) return { error: JSON.stringify(r.body) };
        const f = r.body.stackFrames[0];
        return { id: f.id, name: f.name, line: f.line, pc: Number(f.instructionPointerReference) };
    }
    stops(threads, ms = 5000) { return Promise.all(threads.map((t) => this.event('stopped', t, ms))); }
}

// The run state and PC of the three cores straight from DBGSR (trigger_switch dump).
function dump() {
    const out = execFileSync('C:\\work\\probe-rs\\target\\debug\\examples\\trigger_switch.exe', ['dump']).toString();
    return out.split('\n').filter((l) => /^CPU|TRC[0-2]/.test(l.trim())).map((l) => l.trim());
}
const cpus = (d) => d.filter((l) => l.startsWith('CPU')).map((l) => ({ dbgsr: parseInt(/DBGSR (0x[0-9a-f]+)/.exec(l)[1], 16), pc: parseInt(/PC (0x[0-9a-f]+)/.exec(l)[1], 16) }));
const halted = (c) => ((c.dbgsr >> 1) & 3) !== 0;
const states = (d) => cpus(d).map((c, i) => `CPU${i} ${halted(c) ? 'halted' : 'runs'} ${hex(c.pc)}`).join(' | ');

async function allStop() {
    const s = new Dap();
    const threads = await s.attach(true);
    await sleep(1500);
    check('T-M1 three threads, one per core', threads.length === 3 && threads.map((t) => t.id).join() === '0,1,2', JSON.stringify(threads));
    check('T-M1 the cores run after configurationDone', cpus(dump()).every((c) => !halted(c)), states(dump()));

    // FR-M13: liveWatch reads on the core it names, without halting it.
    const lw = [];
    for (const core of [0, 1, 2, undefined]) {
        const r = await s.send('liveWatch', core === undefined ? { expressions: ['g_result'] } : { expressions: ['g_result'], core });
        lw.push(r.success && r.body.values[0].value !== undefined ? `${core ?? 'focus'}=${r.body.values[0].value}` : `${core}: ${JSON.stringify(r.body || r.message)}`);
    }
    check('FR-M13 liveWatch with and without core, the cores keep running', lw.every((v) => /=\d+$/.test(v)) && cpus(dump()).every((c) => !halted(c)), lw.join(' '));

    // T-M2: a breakpoint in square (CPU0 runs it) halts all three.
    let stops = s.stops([0, 1, 2]);
    const bp = await s.send('setBreakpoints', { source: { path: SRC }, breakpoints: [{ line: 17 }] });
    const b0 = bp.body && bp.body.breakpoints[0];
    check('T-M2 one breakpoint response for the three cores', bp.success && b0 && b0.verified && /cores 0, 1, 2/.test(b0.message || ''), JSON.stringify(b0));
    let st = await stops;
    check('T-M2 CPU0 breakpoint, CPU1 and CPU2 external', st[0] && st[0].reason === 'breakpoint' && st[1] && st[1].reason === 'external' && st[2] && st[2].reason === 'external', st.map((x) => x && x.reason).join(' / '));
    check('T-M2 no event claims all threads stopped', st.every((x) => x && x.allThreadsStopped === false), st.map((x) => x && x.allThreadsStopped).join(' '));
    const d2 = cpus(dump());
    const tops = [await s.top(0), await s.top(1), await s.top(2)];
    check('T-M2 each thread shows the PC of its own core', tops.every((t, i) => t.pc === d2[i].pc), tops.map((t, i) => `${i}: ${hex(t.pc)} (DBGSR PC ${hex(d2[i].pc)})`).join(', '));
    check('T-M2 CPU0 is in square', tops[0].name === 'square' && tops[0].line === 17, `${tops[0].name}:${tops[0].line}`);

    // T-M9: scopes and variables of a frame go to the core of the frame.
    const sc0 = await s.send('scopes', { frameId: tops[0].id });
    const regs0 = sc0.body.scopes.find((x) => /egister/.test(x.name));
    const vars0 = await s.send('variables', { variablesReference: regs0.variablesReference });
    const pc0 = vars0.body.variables.find((v) => /^PC$/i.test(v.name) || /\bpc\b/i.test(v.name));
    const sc1 = await s.send('scopes', { frameId: tops[1].id });
    const regs1 = sc1.body && sc1.body.scopes.find((x) => /egister/.test(x.name));
    const vars1 = regs1 ? await s.send('variables', { variablesReference: regs1.variablesReference }) : { body: { variables: [] } };
    const pc1 = vars1.body.variables.find((v) => /^PC$/i.test(v.name) || /\bpc\b/i.test(v.name));
    check('T-M9 the register scope of each frame reads its own core', pc0 && pc1 && Number(pc0.value) === tops[0].pc && Number(pc1.value) === tops[1].pc, `CPU0 PC ${pc0 && pc0.value}, CPU1 PC ${pc1 && pc1.value}`);
    const ev = await s.send('evaluate', { expression: 'g_result', frameId: tops[0].id, context: 'watch' });
    check('T-M9 evaluate in a CPU0 frame', ev.success && /^\d+$/.test(ev.body.result), ev.success ? ev.body.result : JSON.stringify(ev.body));
    // T-M9: setVariable of a CPU1 register (variablesReference of CPU1) writes CPU1's D0 only.
    const rawD0 = (base) => { const b = execFileSync('C:\\work\\probe-rs\\target\\debug\\examples\\bus_read.exe', [hex(base + 0xFF00), '4']).toString().trim().split(/\s+/).map((x) => parseInt(x, 16)); return (b[0] | b[1] << 8 | b[2] << 16 | b[3] << 24) >>> 0; };
    const d0v = vars1.body.variables.find((v) => /^d0$/i.test(v.name));
    const cpu0Before = rawD0(0xF8810000), cpu1Before = rawD0(0xF8830000);
    const setv = d0v && await s.send('setVariable', { variablesReference: regs1.variablesReference, name: d0v.name, value: '0x12345678' });
    const cpu1After = rawD0(0xF8830000), cpu0After = rawD0(0xF8810000);
    const back = d0v && await s.send('setVariable', { variablesReference: regs1.variablesReference, name: d0v.name, value: hex(cpu1Before) });
    check('T-M9 setVariable of a CPU1 register writes CPU1 only (and back)', setv && setv.success && cpu1After === 0x12345678 && cpu0After === cpu0Before && back.success && rawD0(0xF8830000) === cpu1Before, `CPU1 D0 ${hex(cpu1Before)} -> ${hex(cpu1After)} -> ${hex(rawD0(0xF8830000))}, CPU0 D0 ${hex(cpu0Before)} -> ${hex(cpu0After)}${setv && !setv.success ? ' ' + JSON.stringify(setv.body) : ''}`);

    // T-M3: continue on CPU0 (breakpoint removed) runs all three.
    await s.send('setBreakpoints', { source: { path: SRC }, breakpoints: [] });
    let conts = [1, 2].map((t) => s.event('continued', t, 5000));
    const c0 = await s.send('continue', { threadId: 0 });
    let cs = await Promise.all(conts);
    await sleep(300);
    check('T-M3 continue on CPU0 runs all three', c0.success && cs.every(Boolean) && cpus(dump()).every((c) => !halted(c)), `${cs.map(Boolean)} | ${states(dump())}`);

    // T-M4: a pause on CPU1 halts all.
    stops = s.stops([0, 1, 2]);
    await s.send('pause', { threadId: 1 });
    st = await stops;
    check('T-M4 pause on CPU1: CPU1 pause, the others external', st[1] && st[1].reason === 'pause' && st[0] && st[0].reason === 'external' && st[2] && st[2].reason === 'external', st.map((x) => x && x.reason).join(' / '));

    // T-M5: a step on CPU0 leaves CPU1 and CPU2 where they are.
    const before = cpus(dump());
    const stepped = s.event('stopped', 0, 5000);
    await s.send('next', { threadId: 0, granularity: 'instruction' });
    const sv = await stepped;
    await sleep(300);
    const after = cpus(dump());
    check('T-M5 a step on CPU0 moves CPU0 only', sv && sv.reason === 'step' && after[1].pc === before[1].pc && after[2].pc === before[2].pc && halted(after[1]) && halted(after[2]), `${sv && sv.reason}, CPU0 ${hex(before[0].pc)} -> ${hex(after[0].pc)}, CPU1 ${hex(after[1].pc)}, CPU2 ${hex(after[2].pc)}`);

    // singleThread: a continue of CPU0 alone leaves CPU1 and CPU2 halted; a normal continue of
    // CPU1 then runs CPU1 and CPU2.
    const capabilities = s.capabilities || {};
    const one = await s.send('continue', { threadId: 0, singleThread: true });
    await sleep(400);
    const da = cpus(dump());
    check('singleThread continue runs CPU0 only', one.success && one.body.allThreadsContinued === false && !halted(da[0]) && halted(da[1]) && halted(da[2]) && capabilities.supportsSingleThreadExecutionRequests === true, `capability ${capabilities.supportsSingleThreadExecutionRequests}, ${states(dump())}`);
    const back12 = [1, 2].map((t) => s.event('continued', t, 5000));
    await s.send('continue', { threadId: 1 });
    await Promise.all(back12);
    await sleep(300);
    check('a normal continue of CPU1 then runs CPU1 and CPU2', cpus(dump()).every((c) => !halted(c)), states(dump()));

    // T-M6: CPU0 stays at its breakpoint when CPU1 continues.
    conts = [0, 1, 2].map((t) => s.event('continued', t, 5000));
    await s.send('continue', { threadId: 0 });
    await Promise.all(conts);
    stops = s.stops([0, 1, 2]);
    await s.send('setBreakpoints', { source: { path: SRC }, breakpoints: [{ line: 17 }] });
    st = await stops;
    check('T-M6 breakpoint on CPU0 halts all again', st[0] && st[0].reason === 'breakpoint' && st[1] && st[2], st.map((x) => x && x.reason).join(' / '));
    conts = [1, 2].map((t) => s.event('continued', t, 5000));
    const c1 = await s.send('continue', { threadId: 1 });
    cs = await Promise.all(conts);
    await sleep(500);
    const d6 = cpus(dump());
    check('T-M6 continue on CPU1 runs CPU1 and CPU2, CPU0 stays at its breakpoint', c1.success && c1.body.allThreadsContinued === false && cs.every(Boolean) && halted(d6[0]) && d6[0].pc === 0x8021c008 && !halted(d6[1]) && !halted(d6[2]), `allThreadsContinued ${c1.body && c1.body.allThreadsContinued}, ${states(dump())}`);

    // T-M10: a data breakpoint whose data id names CPU0.
    await s.send('setBreakpoints', { source: { path: SRC }, breakpoints: [] });
    const f0 = await s.top(0);
    const info = await s.send('dataBreakpointInfo', { name: 'g_result', frameId: f0.id });
    check('T-M10 the data id names the core', info.success && /@0$/.test(info.body.dataId || ''), JSON.stringify(info.body));
    const db = await s.send('setDataBreakpoints', { breakpoints: [{ dataId: info.body.dataId, accessType: 'write' }] });
    check('T-M10 data breakpoint verified', db.success && db.body.breakpoints.length === 1 && db.body.breakpoints[0].verified, JSON.stringify(db.body));
    stops = s.stops([0], 8000);
    conts = [1, 2].map((t) => s.event('continued', t, 5000));
    await s.send('continue', { threadId: 0 });
    st = await stops;
    check('T-M10 CPU0 stops at the data breakpoint', st[0] && st[0].reason === 'data breakpoint', st[0] ? st[0].reason : 'no stop');
    await s.send('setDataBreakpoints', { breakpoints: [] });

    // End: everything runs, the routing is put back.
    await s.send('continue', { threadId: 0 });
    await sleep(500);
    await s.send('disconnect', { terminateDebuggee: false });
    await sleep(1500);
    const d = dump();
    console.log('  after disconnect:', d.join(' | '));
    const trc = d.filter((l) => l.startsWith('CBS_TRC')).map((l) => l.split(/\s+/)[2]).join(' ');
    check('routing put back and all cores run after disconnect', trc === '0x00000002 0x00000000 0x00000000' && cpus(d).every((c) => !halted(c)), `${trc} | ${states(d)}`);
}

async function noAllStop() {
    const s = new Dap();
    await s.attach(false);
    await sleep(1500);
    const stop0 = s.event('stopped', 0, 5000);
    const others = [1, 2].map((t) => s.event('stopped', t, 1500));
    await s.send('setBreakpoints', { source: { path: SRC }, breakpoints: [{ line: 17 }] });
    const st0 = await stop0;
    const st12 = await Promise.all(others);
    check('T-M7 without allStop a breakpoint halts CPU0 only', st0 && st0.reason === 'breakpoint' && st12.every((x) => !x) && cpus(dump()).slice(1).every((c) => !halted(c)), `${st0 && st0.reason}, ${states(dump())}`);
    await s.send('setBreakpoints', { source: { path: SRC }, breakpoints: [] });
    const c = await s.send('continue', { threadId: 0 });
    check('T-M7 continue answers for its thread only', c.success && c.body.allThreadsContinued === false, JSON.stringify(c.body));
    const p = s.event('stopped', 2, 5000);
    const others2 = [0, 1].map((t) => s.event('stopped', t, 1500));
    await s.send('pause', { threadId: 2 });
    const sp = await p;
    const so = await Promise.all(others2);
    check('T-M7 pause on CPU2 halts CPU2 only', sp && so.every((x) => !x), `${sp && sp.reason}, ${states(dump())}`);
    await s.send('continue', { threadId: 2 });
    await sleep(500);
    await s.send('disconnect', { terminateDebuggee: false });
    await sleep(1000);
    check('all cores run after disconnect', cpus(dump()).every((c) => !halted(c)), states(dump()));
}

// T-M8: the three cores run the same code (start_in_flash --core 1 / --core 2 before). One
// source breakpoint takes a trigger unit on each core, and whichever core reaches it stops with
// the reason `breakpoint`.
async function sameCode() {
    const s = new Dap();
    await s.attach(true);
    await sleep(1000);
    let first = s.stops([0, 1, 2], 5000);
    const bp = await s.send('setBreakpoints', { source: { path: SRC }, breakpoints: [{ line: 17 }] });
    check('T-M8 one breakpoint, set on the three cores', bp.success && /cores 0, 1, 2/.test(bp.body.breakpoints[0].message || ''), bp.body.breakpoints[0].message);
    // The 8 trigger units of each core (TRnEVT, TRnADR at CSFR base + 0xF000), read off the bus.
    const units = [0xF881F000, 0xF883F000, 0xF885F000].map((base) => {
        const bytes = execFileSync('C:\\work\\probe-rs\\target\\debug\\examples\\bus_read.exe', [hex(base), '64']).toString().trim().split(/\s+/).map((b) => parseInt(b, 16));
        const word = (i) => bytes[i] | bytes[i + 1] << 8 | bytes[i + 2] << 16 | (bytes[i + 3] << 24 >>> 0);
        return [...Array(8).keys()].some((u) => word(8 * u) !== 0 && (word(8 * u + 4) >>> 0) === 0x8021c008);
    });
    check('T-M8 each core has a trigger unit on the breakpoint', units.every(Boolean), units.join(' '));
    const hitBy = new Set();
    for (let round = 0; round < 12 && hitBy.size < 3; round++) {
        const stops = round === 0 ? first : s.stops([0, 1, 2], 5000);
        if (round > 0) await s.send('continue', { threadId: [...s.log].reverse().find((e) => e.event === 'stopped' && e.reason === 'breakpoint').thread });
        const st = await stops;
        st.forEach((x, t) => { if (x && x.reason === 'breakpoint') hitBy.add(t); });
        const tops = await Promise.all([0, 1, 2].map((t) => s.top(t)));
        console.log(`  round ${round}: ${st.map((x, t) => `${t} ${x ? x.reason : '-'} ${hex(tops[t].pc)} ${tops[t].name}`).join(' | ')}`);
        if (process.env.HOLD && round === 0) {
            console.log('HOLD');
            await sleep(Number(process.env.HOLD));
        }
        if (process.env.TRIG_DUMP && round < 3) {
            for (const [i, base] of [0xF881F000, 0xF883F000, 0xF885F000].entries()) {
                const bytes = execFileSync('C:\\work\\probe-rs\\target\\debug\\examples\\bus_read.exe', [hex(base), '64']).toString().trim().split(/\s+/).map((b) => parseInt(b, 16));
                const word = (k) => (bytes[k] | bytes[k + 1] << 8 | bytes[k + 2] << 16 | bytes[k + 3] << 24) >>> 0;
                console.log(`    CPU${i} units: ${[...Array(8).keys()].map((u) => `${hex(word(8 * u))}@${hex(word(8 * u + 4))}`).join(' ')}`);
            }
        }
    }
    check('T-M8 the breakpoint stops more than one core (each with reason breakpoint)', hitBy.size >= 2, [...hitBy].sort().join(', '));
    await s.send('setBreakpoints', { source: { path: SRC }, breakpoints: [] });
    for (const t of [0, 1, 2]) await s.send('continue', { threadId: t });
    await sleep(500);
    await s.send('disconnect', { terminateDebuggee: false });
    await sleep(1000);
    check('T-M8 all cores run after disconnect', cpus(dump()).every((c) => !halted(c)), states(dump()));
}

// The trigger units of each core that hold an address: [[address, ...] per core].
function triggerUnits() {
    return [0xF881F000, 0xF883F000, 0xF885F000].map((base) => {
        const bytes = execFileSync('C:\\work\\probe-rs\\target\\debug\\examples\\bus_read.exe', [hex(base), '64']).toString().trim().split(/\s+/).map((b) => parseInt(b, 16));
        const word = (k) => (bytes[k] | bytes[k + 1] << 8 | bytes[k + 2] << 16 | bytes[k + 3] << 24) >>> 0;
        return [...Array(8).keys()].filter((u) => word(8 * u) !== 0).map((u) => word(8 * u + 4));
    });
}
const trcs = (d) => d.filter((l) => l.startsWith('CBS_TRC')).map((l) => l.split(/\s+/)[2]).join(' ');
const DEFAULT_TRC = '0x00000002 0x00000000 0x00000000';

// T-M11: the application resets the device (sw_reset, as the PF0 application would). The
// breakpoint at the application's start code is kept through the reset on every core, CPU0 halts
// there, the console names the reset, and the routing of the halt group is set again.
async function resetApp() {
    const START = 0x8003cd9a;
    const s = new Dap();
    const said = [];
    const onData = s.onData.bind(s);
    s.onData = (c) => { const t = c.toString(); if (/reset/i.test(t)) said.push(t); onData(c); };
    s.p.stdout.removeAllListeners('data');
    s.p.stdout.on('data', (c) => s.onData(c));
    // allStop on (ALLSTOP=0: off). With the halt state routing of earlier versions, a halt group of
    // two or more cores locked the device on an application reset; the break-out routing does not.
    const allStopOn = process.env.ALLSTOP !== '0';
    await s.attach(allStopOn);
    await sleep(1000);
    const bp = await s.send('setInstructionBreakpoints', { breakpoints: process.env.NOBP ? [] : [{ instructionReference: hex(START) }] });
    check('T-M11 instruction breakpoint at the start code', bp.success && bp.body.breakpoints.every((b) => b.verified), JSON.stringify(bp.body.breakpoints));
    const routed = trcs(dump());
    const stop0 = s.event('stopped', 0, 8000);
    try { console.log('  sw_reset:', execFileSync('C:\\work\\probe-rs\\target\\debug\\examples\\sw_reset.exe', [], { stdio: ['ignore', 'pipe', 'pipe'] }).toString().trim()); } catch (e) { console.log('  sw_reset failed:', String(e.stdout), String(e.stderr)); }
    const st = await stop0;
    const top = await s.top(0);
    await sleep(500);
    const d = dump();
    check('T-M11 CPU0 halts at the start code after the reset', st && st.reason === 'breakpoint' && top.pc === START, `${st && st.reason} ${hex(top.pc)}`);
    check('T-M11 the console names the reset', said.some((t) => /reset by software|was reset/i.test(t)), said.map((t) => (/"output":"([^"]*)/.exec(t) || [])[1]).filter(Boolean).join(' | ').slice(0, 200));
    if (allStopOn) check('T-M11 the halt group routing is set again after the reset', trcs(d) === routed && routed !== DEFAULT_TRC, `before ${routed}, after ${trcs(d)}`);
    const units = triggerUnits();
    check('T-M11 the breakpoint is kept on the three cores', units.every((u) => u.includes(START)), units.map((u, i) => `CPU${i} ${u.map(hex).join(',') || '-'}`).join(' | '));
    await s.send('setInstructionBreakpoints', { breakpoints: [] });
    for (const t of [0, 1, 2]) await s.send('continue', { threadId: t });
    await sleep(500);
    await s.send('disconnect', { terminateDebuggee: false });
    await sleep(1500);
    const e = dump();
    check('T-M11 routing put back, CPU0 runs after disconnect', trcs(e) === DEFAULT_TRC && !halted(cpus(e)[0]), `${trcs(e)} | ${states(e)}`);
}

// T-M12: the DAP server is killed with a breakpoint on the three cores; the next session takes the
// breakpoints and the routing back from the journals of the three cores.
async function killed() {
    const os = require('os'); const fs = require('fs'); const path = require('path');
    const dir = path.join(os.tmpdir(), 'probe-rs-tricore-breakpoints');
    const journals = () => (fs.existsSync(dir) ? fs.readdirSync(dir, { recursive: true }).map((f) => path.basename(String(f))).filter((f) => /^core\d-/.test(f)) : []);
    const before = journals();
    const s = new Dap();
    await s.attach(true);
    await sleep(1000);
    const stops = s.stops([0, 1, 2]);
    await s.send('setBreakpoints', { source: { path: SRC }, breakpoints: [{ line: 17 }] });
    await stops;
    const mine = journals().filter((f) => !before.includes(f));
    check('T-M12 a journal per core', [0, 1, 2].every((c) => mine.some((f) => f.startsWith(`core${c}-`))), mine.join(' '));
    s.p.kill('SIGKILL');
    await sleep(1000);
    const d = dump();
    const left = triggerUnits();
    check('T-M12 killed: routing and trigger units stay', trcs(d) !== DEFAULT_TRC && left.every((u) => u.includes(0x8021c008)), `${trcs(d)} | ${left.map((u, i) => `CPU${i} ${u.map(hex).join(',')}`).join(' | ')}`);
    const n = new Dap();
    await n.attach(true);
    await sleep(1000);
    for (const t of [0, 1, 2]) await n.send('continue', { threadId: t });
    await sleep(500);
    await n.send('disconnect', { terminateDebuggee: false });
    await sleep(1500);
    const e = dump();
    const after = triggerUnits();
    check('T-M12 the next session puts everything back', trcs(e) === DEFAULT_TRC && after.every((u) => !u.includes(0x8021c008)) && cpus(e).every((c) => !halted(c)) && journals().filter((f) => !before.includes(f)).length === 0, `${trcs(e)} | units ${after.map((u) => u.length).join(',')} | ${states(e)} | journals left ${journals().filter((f) => !before.includes(f)).join(' ') || 'none'}`);
}

// T-M11: a restart (device reset by the session); the PF0 application boots and starts CPU1 and
// CPU2. The session routes the three cores again and sets the breakpoint on each again.
async function restart() {
    const s = new Dap();
    await s.attach(true);
    await sleep(1000);
    const stops = s.stops([0, 1, 2]);
    await s.send('setBreakpoints', { source: { path: SRC }, breakpoints: [{ line: 17 }] });
    await stops;
    const r = await s.send('restart', {});
    check('T-M11 restart', r.success, r.success ? 'ok' : JSON.stringify(r.body || r.message));
    await sleep(3000);
    const d = dump();
    console.log('  after the restart:', trcs(d), '|', states(d));
    check('T-M11 the three cores are routed again', trcs(d) === '0x00700070 0x00700070 0x00700070', trcs(d));
    check('T-M11 the three cores run the application', cpus(d).every((c) => !halted(c)), states(d));
    const units = triggerUnits();
    check('T-M11 the breakpoint is set again on the three cores', units.every((u) => u.includes(0x8021c008)), units.map((u, i) => `CPU${i} ${u.map(hex).join(',') || '-'}`).join(' | '));
    const st = s.stops([0, 1, 2]);
    await s.send('pause', { threadId: 1 });
    const ps = await st;
    check('T-M11 pause on CPU1 halts all after the restart', ps.every(Boolean) && ps[1].reason === 'pause', ps.map((x) => x && x.reason).join(' / '));
    await s.send('setBreakpoints', { source: { path: SRC }, breakpoints: [] });
    const conts = [0, 2].map((t) => s.event('continued', t, 5000));
    await s.send('continue', { threadId: 1 });
    const cs = await Promise.all(conts);
    await sleep(300);
    check('T-M11 continue on CPU1 runs all', cs.every(Boolean) && cpus(dump()).every((c) => !halted(c)), states(dump()));
    await s.send('disconnect', { terminateDebuggee: false });
    await sleep(1500);
    const e = dump();
    check('T-M11 routing put back after disconnect', trcs(e) === DEFAULT_TRC, trcs(e));
}

// perfWindow per core in one session: CPU0 counts from square to the return in scale (24
// instructions per compute call, as dap_perf.js), CPU2 has a window that cannot be resolved.
async function perf() {
    const COMPUTE = 0x8021c07e;
    const s = new Dap();
    await s.attach(true, { 0: { perfWindow: { start: 'square', stop: '0x8021c024' } }, 2: { perfWindow: { start: 'nosuchfunction', stop: 'square' } } });
    await sleep(1000);
    const said = s.out.filter((o) => /perfWindow|Performance counters/.test(o)).map((o) => o.trim());
    check('perfWindow messages name their core', said.some((o) => /Performance counters of core 0 count from square/.test(o)) && said.some((o) => /perfWindow` of core 2 is ignored/.test(o)) && !said.some((o) => /core 1/.test(o)), said.join(' | ').slice(0, 300));
    const counters = async (thread) => {
        const top = await s.top(thread);
        const sc = await s.send('scopes', { frameId: top.id });
        const regs = sc.body.scopes.find((x) => /egister/.test(x.name));
        const vars = await s.send('variables', { variablesReference: regs.variablesReference });
        let all = vars.body.variables;
        // The counters may be in a group of their own (Performance).
        for (const v of vars.body.variables.filter((x) => x.variablesReference && /erform/.test(x.name))) {
            all = all.concat((await s.send('variables', { variablesReference: v.variablesReference })).body.variables);
        }
        const value = (name) => { const v = all.find((x) => x.name.toLowerCase() === name); return v ? Number(v.value) : undefined; };
        return { icnt: value('icnt'), cctrl: value('cctrl'), ccnt: value('ccnt') };
    };
    let stops = s.stops([0, 1, 2]);
    await s.send('setInstructionBreakpoints', { breakpoints: [{ instructionReference: hex(COMPUTE) }] });
    await stops;
    const i0 = (await counters(0)).icnt;
    const c1 = await counters(1);
    const runs = [];
    for (let k = 0; k < 2; k++) {
        stops = s.stops([0, 1, 2]);
        await s.send('continue', { threadId: 0 });
        await stops;
        runs.push((await counters(0)).icnt);
    }
    check('CPU0 counts 24 instructions per compute call in its window', runs[0] - i0 === 24 && runs[1] - runs[0] === 24, `icnt ${i0} -> ${runs.join(' -> ')}`);
    check('CPU1 frames show the counters of CPU1', c1.icnt !== undefined && c1.cctrl !== undefined, JSON.stringify(c1));
    await s.send('setInstructionBreakpoints', { breakpoints: [] });
    for (const t of [0, 1, 2]) await s.send('continue', { threadId: t });
    await sleep(500);
    await s.send('disconnect', { terminateDebuggee: false });
    await sleep(1000);
    check('all cores run after disconnect', cpus(dump()).every((c) => !halted(c)), states(dump()));
}

({ noallstop: noAllStop, samecode: sameCode, reset: resetApp, restart, kill: killed, perf }[process.argv[2]] || allStop)()
    .then(() => { console.log(`${results.filter((x) => !x).length} check(s) failed`); setTimeout(() => process.exit(0), 300); })
    .catch((e) => { console.log(`ERROR: ${e.stack}`); console.log(`${results.filter((x) => !x).length} check(s) failed`); process.exit(2); });
