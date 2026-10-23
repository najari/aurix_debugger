// probe-rs Live Watch: reads global variables while the target runs.
//
// Every probe-rs debug session (one per core when a compound configuration starts several) gets
// the `liveWatch` custom request of the probe-rs DAP server, which reads the variables without
// halting the core. With one session the view lists the expressions; with several it groups
// them under each session.
'use strict';

const vscode = require('vscode');

const DEBUG_TYPE = 'probe-rs-debug';
const STATE_KEY = 'probeRsLiveWatch.expressions';
const MIN_INTERVAL = 100;

class LiveWatch {
    constructor(context) {
        this.context = context;
        this.expressions = context.workspaceState.get(STATE_KEY, []);
        /** @type {Map<string, {session: vscode.DebugSession, values: Map<string, object>, busy: boolean, error?: string}>} */
        this.sessions = new Map();
        this.paused = false;
        this.timer = undefined;
        this.changed = new vscode.EventEmitter();
        this.onDidChangeTreeData = this.changed.event;
    }

    // ---- sessions -------------------------------------------------------------------------

    addSession(session) {
        if (session.type !== DEBUG_TYPE) return;
        this.sessions.set(session.id, { session, values: new Map(), busy: false });
        this.updateContext();
        this.schedule();
    }

    removeSession(session) {
        if (this.sessions.delete(session.id)) {
            this.updateContext();
            this.schedule();
            this.changed.fire();
        }
    }

    updateContext() {
        vscode.commands.executeCommand('setContext', 'probeRsLiveWatch.hasSession', this.sessions.size > 0);
        vscode.commands.executeCommand('setContext', 'probeRsLiveWatch.paused', this.paused);
    }

    // ---- expressions ----------------------------------------------------------------------

    save() {
        this.context.workspaceState.update(STATE_KEY, this.expressions);
        this.changed.fire();
        this.refresh();
    }

    add(expression) {
        const name = (expression || '').trim();
        if (name && !this.expressions.includes(name)) {
            this.expressions.push(name);
            this.save();
        }
    }

    remove(expression) {
        this.expressions = this.expressions.filter((e) => e !== expression);
        this.save();
    }

    clear() {
        this.expressions = [];
        this.save();
    }

    // ---- refresh --------------------------------------------------------------------------

    interval() {
        const ms = vscode.workspace.getConfiguration('probeRsLiveWatch').get('intervalMs', 500);
        return Math.max(MIN_INTERVAL, Number(ms) || 500);
    }

    schedule() {
        clearInterval(this.timer);
        this.timer = undefined;
        if (!this.paused && this.sessions.size > 0) {
            this.timer = setInterval(() => this.refresh(), this.interval());
        }
    }

    setPaused(paused) {
        this.paused = paused;
        this.updateContext();
        this.schedule();
        this.changed.fire();
    }

    async refresh() {
        if (this.expressions.length === 0 || (this.view && !this.view.visible)) return;
        await Promise.all([...this.sessions.values()].map((entry) => this.refreshSession(entry)));
    }

    async refreshSession(entry) {
        // A slow probe must not pile up requests.
        if (entry.busy) return;
        entry.busy = true;
        try {
            const body = await entry.session.customRequest('liveWatch', { expressions: this.expressions });
            const values = new Map();
            for (const value of (body && body.values) || []) values.set(value.expression, value);
            for (const [expression, value] of values) {
                const old = entry.values.get(expression);
                value.changed = !!old && old.value !== value.value;
            }
            entry.values = values;
            entry.error = undefined;
        } catch (error) {
            entry.error = error && error.message ? error.message : String(error);
        } finally {
            entry.busy = false;
        }
        this.changed.fire();
    }

    // ---- tree -----------------------------------------------------------------------------

    getChildren(element) {
        const sessions = [...this.sessions.values()];
        if (!element) {
            if (sessions.length > 1) return sessions.map((entry) => ({ kind: 'session', entry }));
            return this.expressions.map((expression) => ({ kind: 'expression', expression, entry: sessions[0] }));
        }
        if (element.kind === 'session') {
            return this.expressions.map((expression) => ({ kind: 'expression', expression, entry: element.entry }));
        }
        return [];
    }

    getTreeItem(element) {
        if (element.kind === 'session') {
            const item = new vscode.TreeItem(element.entry.session.name, vscode.TreeItemCollapsibleState.Expanded);
            item.iconPath = new vscode.ThemeIcon('debug-alt');
            item.description = element.entry.error ? `error: ${element.entry.error}` : '';
            return item;
        }
        const item = new vscode.TreeItem(element.expression, vscode.TreeItemCollapsibleState.None);
        item.contextValue = 'expression';
        const entry = element.entry;
        const value = entry && entry.values.get(element.expression);
        if (!entry) {
            item.description = 'no probe-rs session';
        } else if (entry.error) {
            item.description = `error: ${entry.error}`;
        } else if (!value) {
            item.description = this.paused ? 'paused' : '…';
        } else if (value.value !== undefined) {
            item.description = value.value;
            item.tooltip = `${element.expression}: ${value.type || ''} = ${value.value}`;
        } else {
            item.description = `<${value.error}>`;
        }
        item.iconPath = new vscode.ThemeIcon(
            'symbol-variable',
            value && value.changed ? new vscode.ThemeColor('charts.orange') : undefined,
        );
        return item;
    }
}

function activate(context) {
    const watch = new LiveWatch(context);
    const view = vscode.window.createTreeView('probeRsLiveWatch', { treeDataProvider: watch });
    watch.view = view;

    for (const session of vscode.debug.activeDebugSession ? [vscode.debug.activeDebugSession] : []) {
        watch.addSession(session);
    }
    watch.updateContext();

    context.subscriptions.push(
        view,
        view.onDidChangeVisibility(() => watch.refresh()),
        vscode.debug.onDidStartDebugSession((s) => watch.addSession(s)),
        vscode.debug.onDidTerminateDebugSession((s) => watch.removeSession(s)),
        vscode.workspace.onDidChangeConfiguration((e) => {
            if (e.affectsConfiguration('probeRsLiveWatch.intervalMs')) watch.schedule();
        }),
        vscode.commands.registerCommand('probeRsLiveWatch.add', async () => {
            const expression = await vscode.window.showInputBox({
                prompt: 'Global variable to watch while the target runs',
                placeHolder: 'g_ticks',
            });
            watch.add(expression);
        }),
        vscode.commands.registerCommand('probeRsLiveWatch.addSelection', () => {
            const editor = vscode.window.activeTextEditor;
            if (editor) watch.add(editor.document.getText(editor.selection));
        }),
        vscode.commands.registerCommand('probeRsLiveWatch.remove', (element) => {
            if (element && element.expression) watch.remove(element.expression);
        }),
        vscode.commands.registerCommand('probeRsLiveWatch.clear', () => watch.clear()),
        vscode.commands.registerCommand('probeRsLiveWatch.pause', () => watch.setPaused(true)),
        vscode.commands.registerCommand('probeRsLiveWatch.resume', () => watch.setPaused(false)),
        vscode.commands.registerCommand('probeRsLiveWatch.setInterval', async () => {
            const current = watch.interval();
            const input = await vscode.window.showInputBox({
                prompt: `Refresh interval in milliseconds (at least ${MIN_INTERVAL})`,
                value: String(current),
                validateInput: (text) => (Number(text) >= MIN_INTERVAL ? undefined : `At least ${MIN_INTERVAL} ms`),
            });
            if (input !== undefined) {
                await vscode.workspace
                    .getConfiguration('probeRsLiveWatch')
                    .update('intervalMs', Number(input), vscode.ConfigurationTarget.Workspace);
            }
        }),
        { dispose: () => clearInterval(watch.timer) },
    );
}

function deactivate() {}

module.exports = { activate, deactivate };
