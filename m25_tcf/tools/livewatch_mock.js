// Checks the Live Watch extension's core handling with a stub of the vscode module.
const Module = require('module');
const state = {};
const vscode = {
    EventEmitter: class { constructor() { this.event = () => {}; } fire() {} },
    TreeItem: class { constructor(label) { this.label = label; } },
    ThemeIcon: class { constructor(id) { this.id = id; } },
    ThemeColor: class {},
    TreeItemCollapsibleState: { None: 0, Expanded: 2 },
    commands: { executeCommand: (_c, key, value) => { state[key] = value; } },
    workspace: { getConfiguration: () => ({ get: (_k, d) => d }) },
    window: {},
};
const load = Module._load;
Module._load = (request, ...rest) => (request === 'vscode' ? vscode : load(request, ...rest));
const { LiveWatch } = require('C:/work/claude/vscode/probe-rs-live-watch/extension.js');

const stored = { 'probeRsLiveWatch.expressions': ['g_result'] };
const context = { workspaceState: { get: (k, d) => stored[k] ?? d, update: (k, v) => { stored[k] = v; } } };
let failed = 0;
const check = (what, ok, detail) => { console.log(`[${ok ? 'PASS' : 'FAIL'}] ${what}: ${detail}`); if (!ok) failed++; };

function session(id, name, threads) {
    const requests = [];
    return {
        id, name, type: 'probe-rs-debug', requests,
        async customRequest(command, args) {
            requests.push({ command, args });
            if (command === 'threads') return { threads };
            return { values: [{ expression: 'g_result', value: String(args.core ?? 'focus') }] };
        },
    };
}

(async () => {
    const watch = new LiveWatch(context);
    const multi = session('a', 'TC275 one session', [{ id: 0, name: '0-TC275' }, { id: 1, name: '1-TC275' }, { id: 2, name: '2-TC275' }]);
    watch.addSession(multi);
    await watch.refresh();
    clearInterval(watch.timer);
    const cores = multi.requests.filter((r) => r.command === 'liveWatch').map((r) => r.args.core);
    check('one request per core, with its core', cores.join(',') === '0,1,2', cores.join(','));
    const roots = watch.getChildren();
    check('a group per core', roots.map((r) => watch.getTreeItem(r).label).join(' | ') === 'TC275 one session · 0-TC275 | TC275 one session · 1-TC275 | TC275 one session · 2-TC275', roots.map((r) => watch.getTreeItem(r).label).join(' | '));
    const values = roots.map((r) => watch.getTreeItem(watch.getChildren(r)[0]).description);
    check('each group shows its core\'s value', values.join(',') === '0,1,2', values.join(','));
    check('Select Cores is offered', state['probeRsLiveWatch.multiCore'] === true, String(state['probeRsLiveWatch.multiCore']));

    // Select Cores: CPU1 only.
    vscode.window.showQuickPick = async (items) => items.filter((i) => i.id === 1);
    await watch.selectCores();
    multi.requests.length = 0;
    await watch.refresh();
    clearInterval(watch.timer);
    const after = multi.requests.filter((r) => r.command === 'liveWatch').map((r) => r.args.core);
    check('only the selected core is read', after.join(',') === '1', after.join(','));
    check('the choice is kept', JSON.stringify(stored['probeRsLiveWatch.cores']) === '{"TC275 one session":[1]}', JSON.stringify(stored['probeRsLiveWatch.cores']));
    const flat = watch.getChildren();
    check('one core left: the expressions at the top', flat.length === 1 && flat[0].kind === 'expression' && watch.getTreeItem(flat[0]).description === '1', `${flat.length} ${flat[0] && flat[0].kind}`);
    watch.removeSession(multi);

    // A session of one core: no core argument, no grouping.
    const single = session('b', 'TC275 CPU0', [{ id: 0, name: '0-TC275' }]);
    watch.addSession(single);
    await watch.refresh();
    clearInterval(watch.timer);
    const req = single.requests.find((r) => r.command === 'liveWatch');
    check('one core: no core argument', req && !('core' in req.args), JSON.stringify(req && req.args));
    check('one core: no Select Cores', state['probeRsLiveWatch.multiCore'] === false, String(state['probeRsLiveWatch.multiCore']));
    console.log(`${failed} check(s) failed`);
    process.exit(failed ? 1 : 0);
})();
