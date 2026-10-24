// One-session launch with two program binaries in the same PFLASH sector (a copy of
// demo_flash.elf for CPU1): the launch is refused before anything is written.
'use strict';
const { spawn } = require('child_process');
const fs = require('fs');
const path = require('path');
const DEMO = 'C:\\work\\aurix_tcf_demo';
const ELF = `${DEMO}\\build\\demo_flash.elf`;
const COPY = path.join(__dirname, 'demo_flash_copy.elf');
const PROBE_RS = 'C:\\work\\probe-rs\\target\\debug\\probe-rs.exe';
fs.copyFileSync(ELF, COPY);

const p = spawn(PROBE_RS, ['dap-server'], { stdio: ['pipe', 'pipe', 'inherit'] });
let seq = 1, buf = Buffer.alloc(0);
const pending = new Map(), out = [];
p.stdout.on('data', (c) => {
    buf = Buffer.concat([buf, c]);
    for (;;) {
        const sep = buf.indexOf('\r\n\r\n'); if (sep < 0) return;
        const len = Number(/Content-Length: (\d+)/.exec(buf.slice(0, sep).toString())[1]);
        if (buf.length < sep + 4 + len) return;
        const m = JSON.parse(buf.slice(sep + 4, sep + 4 + len).toString()); buf = buf.slice(sep + 4 + len);
        if (m.type === 'response') { const r = pending.get(m.request_seq); pending.delete(m.request_seq); if (r) r(m); }
        else if (m.event === 'output' || m.event === 'progressStart') out.push(JSON.stringify(m.body));
    }
});
const send = (command, args) => new Promise((r) => { const msg = { seq: seq++, type: 'request', command, arguments: args }; const b = Buffer.from(JSON.stringify(msg)); p.stdin.write(`Content-Length: ${b.length}\r\n\r\n`); p.stdin.write(b); pending.set(msg.seq, r); });

(async () => {
    await send('initialize', { adapterID: 'probe-rs-debug', linesStartAt1: true, columnsStartAt1: true, pathFormat: 'path' });
    const r = await Promise.race([
        send('launch', {
            cwd: DEMO, chip: 'TC275', probe: '058b:0000', allStop: true,
            flashingConfig: { flashingEnabled: true },
            coreConfigs: [{ coreIndex: 0, programBinary: ELF }, { coreIndex: 1, programBinary: COPY }],
            consoleLogLevel: 'Console',
        }),
        new Promise((r) => setTimeout(() => r(null), 60000)),
    ]);
    const message = r ? JSON.stringify(r.body || r.message) : 'no response';
    const flashed = out.some((o) => /Flashing device|Programming/.test(o));
    const ok = r && !r.success && /both (write the flash sector at 0xa021c000|load 0x8021c000)/.test(message) && !flashed;
    console.log(`[${ok ? 'PASS' : 'FAIL'}] two binaries in one PFLASH sector are refused before the write: ${message.slice(0, 300)} | flash started: ${flashed}`);
    console.log(`${ok ? 0 : 1} check(s) failed`);
    p.kill();
    process.exit(0);
})();
