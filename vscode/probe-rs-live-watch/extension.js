// probe-rs Live Watch: reads global variables while the target runs.
//
// Every probe-rs debug session gets the `liveWatch` custom request of the probe-rs DAP server,
// which reads the variables without halting the core. A session can debug several cores (one
// thread per core); `liveWatch` then takes the core to read with (`core`). The view reads every
// selected core of every session: with one, it lists the expressions; with several, it groups
// them under each session and core. "Select Cores" picks the cores of a session to read.
'use strict';

const vscode = require('vscode');

const DEBUG_TYPE = 'probe-rs-debug';
const STATE_KEY = 'probeRsLiveWatch.expressions';
const CORES_KEY = 'probeRsLiveWatch.cores';
const MIN_INTERVAL = 100;

class LiveWatch {
    constructor(context) {
        this.context = context;
        this.expressions = context.workspaceState.get(STATE_KEY, []);
        /** Selected cores by session name: core ids. A session not listed reads all its cores. */
        this.selectedCores = context.workspaceState.get(CORES_KEY, {});
        /** @type {Map<string, {session: vscode.DebugSession, cores?: {id: number, name: string}[], probing: boolean}>} */
        this.sessions = new Map();
        /** What the view reads: a session, or one core of a session.
         * @type {Map<string, {session: vscode.DebugSession, core?: number, label: string, values: Map<string, object>, busy: boolean, error?: string}>} */
        this.targets = new Map();
        this.paused = false;
        this.timer = undefined;
        this.changed = new vscode.EventEmitter();
        this.onDidChangeTreeData = this.changed.event;
    }

    // ---- sessions -------------------------------------------------------------------------

    addSession(session) {
        if (session.type !== DEBUG_TYPE) return;
        this.sessions.set(session.id, { session, cores: undefined, probing: false });
        this.syncTargets();
        this.schedule();
    }

    removeSession(session) {
        if (this.sessions.delete(session.id)) {
            this.syncTargets();
            this.schedule();
        }
    }

    // The cores of a session are its threads, when it has several. Asked again until the session
    // answers (it may not be configured yet).
    async discoverCores(entry) {
        if (entry.cores !== undefined || entry.probing) return;
        entry.probing = true;
        try {
            const body = await entry.session.customRequest('threads');
            const threads = (body && body.threads) || [];
            if (threads.length > 0) {
                entry.cores = threads.length > 1 ? threads.map((t) => ({ id: t.id, name: t.name })) : [];
            }
        } catch {
            // Asked again at the next refresh.
        } finally {
            entry.probing = false;
        }
        if (entry.cores !== undefined) this.syncTargets();
    }

    coresToRead(entry) {
        const selected = this.selectedCores[entry.session.name];
        return entry.cores.filter((core) => !Array.isArray(selected) || selected.includes(core.id));
    }

    syncTargets() {
        const targets = new Map();
        for (const entry of this.sessions.values()) {
            if (entry.cores && entry.cores.length > 0) {
                for (const core of this.coresToRead(entry)) {
                    const key = `${entry.session.id}/${core.id}`;
                    targets.set(key, this.targets.get(key) || {
                        session: entry.session,
                        core: core.id,
                        label: `${entry.session.name} · ${core.name}`,
                        values: new Map(),
                        busy: false,
                    });
                }
            } else {
                const key = entry.session.id;
                targets.set(key, this.targets.get(key) || {
                    session: entry.session,
                    label: entry.session.name,
                    values: new Map(),
                    busy: false,
                });
            }
        }
        this.targets = targets;
        this.updateContext();
        this.changed.fire();
    }

    multiCoreSessions() {
        return [...this.sessions.values()].filter((entry) => entry.cores && entry.cores.length > 0);
    }

    updateContext() {
        vscode.commands.executeCommand('setContext', 'probeRsLiveWatch.hasSession', this.sessions.size > 0);
        vscode.commands.executeCommand('setContext', 'probeRsLiveWatch.multiCore', this.multiCoreSessions().length > 0);
        vscode.commands.executeCommand('setContext', 'probeRsLiveWatch.paused', this.paused);
    }

    async selectCores() {
        const sessions = this.multiCoreSessions();
        if (sessions.length === 0) {
            vscode.window.showInformationMessage('No probe-rs session debugs several cores.');
            return;
        }
        let entry = sessions[0];
        if (sessions.length > 1) {
            const pick = await vscode.window.showQuickPick(
                sessions.map((s) => ({ label: s.session.name, entry: s })),
                { placeHolder: 'Session' },
            );
            if (!pick) return;
            entry = pick.entry;
        }
        const reading = new Set(this.coresToRead(entry).map((core) => core.id));
        const picks = await vscode.window.showQuickPick(
            entry.cores.map((core) => ({ label: core.name, description: `core ${core.id}`, picked: reading.has(core.id), id: core.id })),
            { canPickMany: true, placeHolder: 'Cores to read the expressions on' },
        );
        if (!picks) return;
        this.selectedCores = { ...this.selectedCores, [entry.session.name]: picks.map((p) => p.id) };
        this.context.workspaceState.update(CORES_KEY, this.selectedCores);
        this.syncTargets();
        await this.refresh();
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
        await Promise.all([...this.sessions.values()].map((entry) => this.discoverCores(entry)));
        if (this.expressions.length === 0 || (this.view && !this.view.visible)) return;
        await Promise.all([...this.targets.values()].map((target) => this.refreshTarget(target)));
    }

    async refreshTarget(target) {
        // A slow probe must not pile up requests.
        if (target.busy) return;
        target.busy = true;
        try {
            const args = { expressions: this.expressions };
            if (target.core !== undefined) args.core = target.core;
            const body = await target.session.customRequest('liveWatch', args);
            const values = new Map();
            for (const value of (body && body.values) || []) values.set(value.expression, value);
            for (const [expression, value] of values) {
                const old = target.values.get(expression);
                value.changed = !!old && old.value !== value.value;
            }
            target.values = values;
            target.error = undefined;
        } catch (error) {
            target.error = error && error.message ? error.message : String(error);
        } finally {
            target.busy = false;
        }
        this.changed.fire();
    }

    // ---- tree -----------------------------------------------------------------------------

    getChildren(element) {
        const targets = [...this.targets.values()];
        if (!element) {
            if (targets.length > 1) return targets.map((target) => ({ kind: 'target', target }));
            return this.expressions.map((expression) => ({ kind: 'expression', expression, target: targets[0] }));
        }
        if (element.kind === 'target') {
            return this.expressions.map((expression) => ({ kind: 'expression', expression, target: element.target }));
        }
        return [];
    }

    getTreeItem(element) {
        if (element.kind === 'target') {
            const item = new vscode.TreeItem(element.target.label, vscode.TreeItemCollapsibleState.Expanded);
            item.iconPath = new vscode.ThemeIcon(element.target.core === undefined ? 'debug-alt' : 'circuit-board');
            item.description = element.target.error ? `error: ${element.target.error}` : '';
            return item;
        }
        const item = new vscode.TreeItem(element.expression, vscode.TreeItemCollapsibleState.None);
        item.contextValue = 'expression';
        const target = element.target;
        const value = target && target.values.get(element.expression);
        if (!target) {
            item.description = this.sessions.size > 0 ? 'no core selected' : 'no probe-rs session';
        } else if (target.error) {
            item.description = `error: ${target.error}`;
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
        vscode.commands.registerCommand('probeRsLiveWatch.selectCores', () => watch.selectCores()),
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

module.exports = { activate, deactivate, LiveWatch };
