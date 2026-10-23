# probe-rs Live Watch

A VS Code view that refreshes global variables while the target runs, for
probe-rs debug sessions (`probe-rs-debug`). It sends the `liveWatch` custom
request of the probe-rs DAP server in the TriCore port
([najari/probe-rs `tricore`](https://github.com/najari/probe-rs/tree/tricore)),
which reads the variables without halting the core.

- **Live Watch** appears in the Run and Debug view while a probe-rs
  session runs. With several sessions (one per core, for example the
  `CPU0 + CPU1 + CPU2` compound configuration) the expressions are grouped
  under each session.
- One session can debug several cores (several `coreConfigs`, one thread
  per core). The view then reads the expressions on each core and groups
  them under `session · core`. **Select Cores** (chip button) picks the
  cores of a session to read; the choice is kept per workspace.
- Add an expression with **+**, or select a name in the editor and choose
  **Add to Live Watch (probe-rs)**. The list is kept per workspace.
- The refresh interval is `probeRsLiveWatch.intervalMs` (default 500 ms, at
  least 100 ms); set it from the view's clock button. A value that changed
  since the last refresh has an orange icon. Pause stops the requests.
- Values are read only while the view is visible, and a session never has
  more than one request in flight.

Only global (static) variables qualify: a running core has no stack frames.
The variable must be in the program binary of the core's configuration
(`programBinary`).

## Install

```
npx @vscode/vsce package
code --install-extension probe-rs-live-watch-0.2.0.vsix
```

The extension has no build step (plain JavaScript) and no dependencies.

## Request

`core` (optional) names the core of a session that debugs several cores
(the thread id); without it the request reads on the focus core.

```json
{ "command": "liveWatch", "arguments": { "expressions": ["g_ticks", "g_result"], "core": 1 } }
→ { "values": [ { "expression": "g_ticks", "value": "1408529", "type": "volatile unsigned int" },
                { "expression": "nope", "error": "not a global variable" } ] }
```
