# Shared debugger crates

The logic of the views the Zed fork prototype (A) added, without Zed: the standalone GPUI
debugger (B) and any other client of the probe-rs debug adapter use it. No crate here depends
on a Zed crate (Zed's `debugger_ui`, `dap`, `ui` and `editor` are GPL-3.0-or-later); the only
dependencies are serde, serde_json and [dap-types](https://github.com/zed-industries/dap-types)
(MIT OR Apache-2.0, the revision Zed uses, so the types are the same as in the Zed fork).

| Crate | What it holds |
| --- | --- |
| `aurix-debug-model` | State without I/O or UI: `reference` (addresses, the probe-rs `@core` suffix), `protocol` (probe-rs `liveWatch`, `rttWindowOpened`, the RTT events, `disassemble` with signed offsets), `disassembly` (the window around a program counter, read further on either side), `breakpoints` (instruction breakpoints, and the condition, hit count and log message of any breakpoint), `source_breakpoints` (source line breakpoints per file, with their conditions), `variables` (scopes and variables as a tree read on demand, expanded nodes kept across stops, changed values marked), `live_watch` (expressions read on every core), `memory` (a memory window read again and again through one core, changed bytes marked, base64; writes only to the TC27x RAMs), `rtt` (RTT lines per core and channel, the down channels and the `rtt write` command that sends a line to one) |
| `aurix-dap-client` | A DAP client over TCP: requests typed with dap-types, events on a channel; threads only, no async runtime |

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

`break_when` sets one breakpoint with a condition (`if=`), hit count (`hit=`) or log message
(`log=`) on a source line or a variable (`data:g_result`), continues at each stop and prints it
with the values of `show=`; `from=<file>:<line>` stops there first so that the hits count from a
known place; `running=g_ticks` tells at the end whether each core still runs;
`tc275_demo_3cores_allstop.json` is the same session with all-stop.

```text
cargo test
cargo clippy --all-targets -- -D warnings
```
