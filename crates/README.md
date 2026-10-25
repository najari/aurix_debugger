# Shared debugger crates

The logic of the views the Zed fork prototype (A) added, without Zed: the standalone GPUI
debugger (B) and any other client of the probe-rs debug adapter use it. No crate here depends
on a Zed crate (Zed's `debugger_ui`, `dap`, `ui` and `editor` are GPL-3.0-or-later); the only
dependencies are serde, serde_json and [dap-types](https://github.com/zed-industries/dap-types)
(MIT OR Apache-2.0, the revision Zed uses, so the types are the same as in the Zed fork).

| Crate | What it holds |
| --- | --- |
| `aurix-debug-model` | State without I/O or UI: `reference` (addresses, the probe-rs `@core` suffix), `protocol` (probe-rs `liveWatch`, `rttWindowOpened`, the RTT events, `disassemble` with signed offsets), `disassembly` (the window around a program counter, read further on either side), `breakpoints` (instruction breakpoints, and the condition, hit count and log message of any breakpoint), `source_breakpoints` (source line breakpoints per file, with their conditions), `variables` (scopes and variables as a tree read on demand, expanded nodes kept across stops, changed values marked), `live_watch` (expressions read on every core), `memory` (a memory window read again and again through one core, changed bytes marked, base64; writes only to the TC27x RAMs), `rtt` (RTT lines per core and channel, the down channels and the `rtt write` command that sends a line to one), `orti` (an ORTI 2.1/2.2 file: the attribute declarations with their ENUM items, the OS objects; what it does not know is skipped with a warning; `OsView` turns the values read into a table per object kind, ENUM values by name, changed values marked), `path_expr` (the C paths the probe-rs fork reads: names, `a.b`, `a[n]`, `p->b`, `*p`) |
| `aurix-dap-client` | A DAP client over TCP: requests typed with dap-types, events on a channel; threads only, no async runtime |
| `aurix-script` | Board tests from a command file without a screen: the `aurix-run` tool (below) |

The license of these crates is not decided yet (`publish = false`).

## Examples on the board

Start the probe-rs fork's server first (`probe-rs dap-server --port 50000`; `DAP_SERVER` names
another address).

```text
cargo run --example rtt_tail -- dap-client/examples/tc275_rtt_cpu0_cpu1.json 5
cargo run --example live_watch -- dap-client/examples/tc275_demo_3cores.json 5 g_ticks g_result
cargo run --example break_when -- dap-client/examples/tc275_demo_3cores.json 10 C:\work\aurix_tcf_demo\src\demo.c:23 "if=v == 3" show=v,k stops=10
```

`rtt_tail` needs the RTT test program on CPU0 and CPU1 (`zed/aurix_tcf_demo/build_rtt.ps1`,
`-Core 1`, started with `start_in_flash`, RAM only); `live_watch` and `break_when` the flash
demo started with `start_in_flash`. They leave the target halted: reset it afterwards.

`dap_measure` times the requests a debugger makes (attach, stepIn, next, stepOut, continue to
a breakpoint, stackTrace, scopes, variables, readMemory 64 B and 4 KiB, liveWatch on every core),
each 20 times, median and 95th percentile:

```text
cargo run --release --example dap_measure -- dap-client/examples/tc275_demo_3cores.json C:\work\aurix_tcf_demo\src\demo.c:22
```

The attach files here set `"speed": 6000`: the probe-rs fork's TAS probe sets the clock of the
DAS JTAG adapter from it (the TAS server starts at 400 kHz; the ShieldBuddy's adapter takes up
to 6 MHz, which makes memory 6.7 times and steps about 3 times faster). Leave it out to keep the
server's clock.

`break_when` sets one breakpoint with a condition (`if=`), hit count (`hit=`) or log message
(`log=`) on a source line or a variable (`data:g_result`), continues at each stop and prints it
with the values of `show=`; `from=<file>:<line>` stops there first so that the hits count from a
known place; `running=g_ticks` tells at the end whether each core still runs;
`tc275_demo_3cores_allstop.json` is the same session with all-stop.

## aurix-run: board tests from a script

```text
cargo run -p aurix-script --bin aurix-run -- script/examples/demo_flash.aurix
aurix-run <script> [--server HOST:PORT] [--keep-going] [--junit FILE] [--set NAME=VALUE]... [--verbose]
```

One command a line, `#` starts a comment, times are `500ms` or `5s`, paths are relative to the
script. `${NAME}` takes the value of `--set NAME=…`, else of the environment variable, else the
default of `${NAME:-default}`. The whole file is read first: a mistake ends the run before the
server is asked anything.

| Command | What it does |
| --- | --- |
| `server <host>:<port>` | The server (default `127.0.0.1:50000`), before `attach` |
| `attach <attach.json>` | initialize, attach, configurationDone (the cores are left as they are). An attach file that turns flashing on is refused |
| `break <file>:<line> [if <condition>] [hit <count>] [log <message>]` | A source breakpoint, with the probe-rs conditions (`v == 3 && k != 0`, `3`, `>= 3`, `% 4`) |
| `unbreak <file>:<line>` | Removes that source breakpoint (the file's others stay) |
| `break-at <address>[@core]` | An instruction breakpoint |
| `watch <variable> read\|write\|access [if …] [hit …]` | A data breakpoint (needs a halted core for the variable's frame) |
| `continue [core\|all]`, `pause [core\|all]` | Without a core: the core that stopped last, else every core |
| `step over\|into\|out\|instruction [core]` | One step |
| `wait stopped [core] [reason <reason>] [within <time>]` | Waits for a stop since the last run command (default 10 s) |
| `expect <name> <op> <value>` | `evaluate` in the top frame of the stopped core, compared like the conditions |
| `expect live <global>[@core] <op> <value> [within <time>]` | `liveWatch` every 0.5 s until it holds |
| `expect memory <address>[@core] <bytes>` | `readMemory` compared |
| `write <address>[@core] <bytes>` | `writeMemory`, RAM of the TC27x only |
| `rtt send <core> <channel> <text>` | A line to the core's RTT down channel |
| `rtt expect <core> <channel> "<regex>" [within <time>]` | A line on the up channel that matches, after the last one matched |
| `sleep <time>`, `disconnect` | The run ends with `disconnect` anyway |

Exit code 0: every expectation held; 1: one did not (the run stops there, or goes on with
`--keep-going` and lists them); 2: a mistake in the script, the server, a wait that ran out of
time, or Ctrl+C. `--junit` writes the script as a testsuite and each expectation as a testcase
(the ones not reached are skipped). Examples in `script/examples`: `demo_flash`, `conditions`
and `data_breakpoint` (flash demo), `rtt_echo` (RTT test program on CPU0 and CPU1), and
`watch_after_break`, a data breakpoint next to an instruction breakpoint (it did not halt before
probe-rs fork `149db65`), and `orti_paths` (the ORTI demo `zed/aurix_tcf_demo/build_orti.ps1`,
RAM only): `expect` and `expect live` on paths such as `OsTcb[1].prio` and
`OsRunningTcb->state`, and a breakpoint for one task, `if OsRunningTcb->prio == 2` (probe-rs fork
`d8954dd`), and `step_over_loop` (flash demo): step over through a `for` loop whose condition
sits after the body must stop in the next iteration (before probe-rs fork `c5d7411` it ran to the
end of the loop).

```text
cargo test
cargo clippy --all-targets -- -D warnings
```
