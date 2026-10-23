// Step Into check: CPU0 at main of demo_flash.elf (start_in_flash). From each call line, a
// stepIn should stop at the first body line of the callee (after its prologue), as GDB does:
// main:45 -> compute:38, compute:38 -> accumulate:29, accumulate:32 -> scale:22, scale:22 -> square:17.
// Then stepOut and stepOver for comparison. `node dap_stepin.js ram` uses demo.elf in the PSPR.
'use strict';
const { spawn } = require('child_process');
const DEMO = 'C:\\work\\aurix_tcf_demo';
const RAM = process.argv[2] === 'ram';
const ELF = `${DEMO}\\build\\${RAM ? 'demo.elf' : 'demo_flash.elf'}`;
const BASE = RAM ? 0x70100000 : 0x8021c000;
const PROBE_RS = 'C:\\work\\probe-rs\\target\\debug\\probe-rs.exe';
const results = [];
const check = (what, ok, detail) => { results.push(ok); console.log(`[${ok ? 'PASS' : 'FAIL'}] ${what}: ${detail}`); };
const hex = (v) => `0x${v.toString(16)}`;

class Dap {
    constructor() {
        this.p = spawn(PROBE_RS, ['dap-server'], { stdio: ['pipe', 'pipe', 'inherit'] });
        this.seq = 1; this.buf = Buffer.alloc(0); this.pending = new Map(); this.waiters = [];
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
            if (m.event === 'output' && /rror|arn/.test(m.body.output)) console.log(`  [output] ${m.body.output.trim().slice(0, 200)}`);
            const i = this.waiters.findIndex((w) => w.name === m.event); if (i >= 0) this.waiters.splice(i, 1)[0].resolve(m.body);
        }
    }
    send(command, args) {
        return new Promise((r) => { const msg = { seq: this.seq++, type: 'request', command, arguments: args }; const b = Buffer.from(JSON.stringify(msg)); this.p.stdin.write(`Content-Length: ${b.length}\r\n\r\n`); this.p.stdin.write(b); this.pending.set(msg.seq, r); });
    }
    event(name, ms) {
        return new Promise((resolve) => { const w = { name, resolve: (e) => { clearTimeout(t); resolve(e); } }; const t = setTimeout(() => { const i = this.waiters.indexOf(w); if (i >= 0) this.waiters.splice(i, 1); resolve(null); }, ms); this.waiters.push(w); });
    }
    async where() {
        const f = (await this.send('stackTrace', { threadId: this.thread, startFrame: 0, levels: 1 })).body.stackFrames[0];
        return { name: f.name, line: f.line, pc: Number(f.instructionPointerReference) };
    }
    async go(command, args = {}) {
        const s = this.event('stopped', 8000);
        const r = await this.send(command, { threadId: this.thread, ...args });
        const e = await s;
        return e ? { reason: e.reason, ...(await this.where()) } : { reason: r.success ? 'no stop' : r.message };
    }
}

const now = () => Number(process.hrtime.bigint()) / 1e6;
const stats = {};
const rec = (k, ms) => (stats[k] = stats[k] || []).push(ms);
const pct = (a, p) => a[Math.min(a.length - 1, Math.floor(a.length * p))];
const show = () => {
    for (const [k, v] of Object.entries(stats)) {
        const a = [...v].sort((x, y) => x - y);
        console.log(`${k.padEnd(34)} n=${String(a.length).padEnd(3)} min ${a[0].toFixed(1).padStart(7)}  median ${pct(a, 0.5).toFixed(1).padStart(7)}  p95 ${pct(a, 0.95).toFixed(1).padStart(7)}  max ${a[a.length - 1].toFixed(1).padStart(7)} ms`);
    }
};
// A timed step: request until the stopped event arrives (the stack trace that follows is timed on its own).
async function timed(s, key, command, args = {}) {
    const stopped = s.event('stopped', 8000);
    const t0 = now();
    await s.send(command, { threadId: s.thread, ...args });
    const e = await stopped;
    rec(key, now() - t0);
    return e;
}
async function call(s, key, command, args) {
    const t0 = now();
    const r = await s.send(command, args);
    rec(key, now() - t0);
    return r;
}

async function main() {
    const s = new Dap();
    await s.send('initialize', { adapterID: 'probe-rs-debug', linesStartAt1: true, columnsStartAt1: true, pathFormat: 'path' });
    const initialized = s.event('initialized', 60000);
    const t0 = now();
    const r = await s.send('attach', { cwd: DEMO, chip: 'TC275', probe: '058b:0000', coreConfigs: [{ coreIndex: 0, programBinary: ELF }], consoleLogLevel: 'Console' });
    if (!r.success) throw new Error(r.message);
    await initialized;
    rec('attach (to initialized)', now() - t0);
    await s.send('configurationDone', {});
    s.thread = (await s.send('threads', {})).body.threads[0].id;
    const first = s.event('stopped', 3000);
    await s.send('setInstructionBreakpoints', { breakpoints: [{ instructionReference: hex(BASE + 0xac) }] });
    if (!(await first)) await timed(s, 'continue (first, to main)', 'continue');
    await s.send('setInstructionBreakpoints', { breakpoints: [] });

    // Stack, variables, evaluate and memory while halted.
    let frame;
    for (let i = 0; i < 20; i++) {
        const st = await call(s, 'stackTrace (1 frame)', 'stackTrace', { threadId: s.thread, startFrame: 0, levels: 1 });
        frame = st.body.stackFrames[0];
    }
    await call(s, 'stackTrace (20 levels)', 'stackTrace', { threadId: s.thread, startFrame: 0, levels: 20 });
    for (let i = 0; i < 10; i++) {
        const sc = await call(s, 'scopes', 'scopes', { frameId: frame.id });
        for (const scope of sc.body.scopes.slice(0, 1)) await call(s, 'variables (first scope)', 'variables', { variablesReference: scope.variablesReference });
    }
    for (let i = 0; i < 20; i++) await call(s, 'evaluate g_result', 'evaluate', { expression: 'g_result', frameId: frame.id, context: 'watch' });
    for (let i = 0; i < 3; i++) await call(s, 'readMemory 4 KiB', 'readMemory', { memoryReference: '0x7000a000', count: 4096 });
    for (let i = 0; i < 5; i++) await call(s, 'readMemory 64 B', 'readMemory', { memoryReference: '0x7000a000', count: 64 });

    // Steps: main -> compute -> accumulate -> scale -> square, then out.
    await timed(s, 'stepIn', 'stepIn');
    await timed(s, 'stepIn', 'stepIn');
    for (let i = 0; i < 4; i++) await timed(s, 'next (source line)', 'next');
    await timed(s, 'stepIn', 'stepIn');
    await timed(s, 'stepIn', 'stepIn');
    await timed(s, 'stepOut', 'stepOut');
    for (let i = 0; i < 6; i++) await timed(s, 'next (source line)', 'next');
    for (let i = 0; i < 6; i++) await timed(s, 'stepIn', 'stepIn');
    await call(s, 'stepInstruction', 'stepInstruction', { threadId: s.thread }).then(() => s.event('stopped', 1000));
    for (let i = 0; i < 10; i++) await timed(s, 'next instruction (granularity)', 'next', { granularity: 'instruction' });

    // Continue to a breakpoint: square is called 4 times per compute call, every pass hits it.
    await s.send('setInstructionBreakpoints', { breakpoints: [{ instructionReference: hex(BASE + 0x08) }] });
    for (let i = 0; i < 10; i++) await timed(s, 'continue to breakpoint', 'continue');
    await s.send('setInstructionBreakpoints', { breakpoints: [] });
    const c = s.event('continued', 3000);
    await s.send('continue', { threadId: s.thread });
    await c;
    await s.send('disconnect', { terminateDebuggee: false });
}

main()
    .then(() => { show(); setTimeout(() => process.exit(0), 300); })
    .catch((e) => { show(); console.log(`ERROR: ${e.stack}`); process.exit(2); });