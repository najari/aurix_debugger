// Minimal TCF client shared by the demo tools (agent at 127.0.0.1:1534).
'use strict';
const net = require('net');
const EOM = Buffer.from([3, 1]);

function encode(words, args) {
    const parts = [Buffer.from(words.join('\0') + '\0', 'utf8')];
    for (const a of (args || [])) {
        parts.push(Buffer.from(JSON.stringify(a), 'utf8'));
        parts.push(Buffer.from([0]));
    }
    parts.push(EOM);
    return Buffer.concat(parts);
}

function connect(port = 1534, host = '127.0.0.1') {
    return new Promise((resolve, reject) => {
        const pending = new Map();
        let token = 0;
        let buf = Buffer.alloc(0);
        const sock = net.connect(port, host, () => {
            sock.write(encode(['E', 'Locator', 'Hello', JSON.stringify(['Locator'])]));
            setTimeout(() => resolve(client), 300);
        });
        sock.on('error', reject);
        sock.on('data', (d) => {
            buf = Buffer.concat([buf, d]);
            for (;;) {
                const i = buf.indexOf(EOM);
                if (i < 0) return;
                const p = buf.subarray(0, i).toString().split('\0');
                buf = buf.subarray(i + 2);
                if (p[0] !== 'R') continue;
                const pr = pending.get(p[1]);
                if (!pr) continue;
                pending.delete(p[1]);
                pr(p.slice(2));
            }
        });
        // send a command; resolves with the raw reply fields (after the token)
        const raw = (service, name, ...args) => new Promise((res) => {
            const t = String(++token);
            pending.set(t, res);
            sock.write(encode(['C', t, service, name], args));
        });
        const fail = (f) => {
            if (!f || f === 'null') return null;
            try { const e = JSON.parse(f); return e && e.Format ? e.Format : f; } catch (x) { return f; }
        };
        const client = {
            raw,
            close: () => sock.end(),
            // Registers.get -> uint32
            async getReg(id) {
                const r = await raw('Registers', 'get', id);
                if (fail(r[0])) throw new Error(`Registers.get ${id}: ${fail(r[0])}`);
                return Buffer.from(JSON.parse(r[1]), 'base64').readUInt32LE(0);
            },
            async setReg(id, v) {
                const b = Buffer.alloc(4); b.writeUInt32LE(v >>> 0, 0);
                const r = await raw('Registers', 'set', id, b.toString('base64'));
                if (fail(r[0])) throw new Error(`Registers.set ${id}: ${fail(r[0])}`);
            },
            // name -> register context id ("PC" -> "R0.core0")
            async regIds(ctx) {
                const r = await raw('Registers', 'getChildren', ctx);
                const ids = JSON.parse(r[1] || '[]');
                const map = {};
                for (const id of ids) {
                    const c = await raw('Registers', 'getContext', id);
                    const def = JSON.parse(c[1] || 'null');
                    if (def && def.Name) map[def.Name] = id;
                }
                return { ids, map };
            },
            async readMem(ctx, addr, size) {
                const r = await raw('Memory', 'get', ctx, addr, 1, size, 0);
                return Buffer.from(JSON.parse(r[0] || '""'), 'base64');
            },
            async writeMem(ctx, addr, data) {
                const r = await raw('Memory', 'set', ctx, addr, 1, data.length, 0, data.toString('base64'));
                if (r[0] && r[0] !== 'null' && r[0] !== '') throw new Error(`Memory.set @0x${addr.toString(16)}: ${r[0]}`);
            },
            async state(ctx) {
                const r = await raw('RunControl', 'getState', ctx);
                return { suspended: r[1] === 'true', pc: Number(r[2]) >>> 0, reason: r[3] ? JSON.parse(r[3]) : null };
            },
            async suspend(ctx) { return raw('RunControl', 'suspend', ctx); },
        };
    });
}

module.exports = { connect };
