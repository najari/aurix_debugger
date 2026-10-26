# Shared debugger crates

The logic of the views the Zed fork prototype (A) added, without Zed: the standalone GPUI
debugger (B) and any other client of the probe-rs debug adapter use it. No crate here depends
on a Zed crate (Zed's `debugger_ui`, `dap`, `ui` and `editor` are GPL-3.0-or-later); the only
dependencies are serde, serde_json and [dap-types](https://github.com/zed-industries/dap-types)
(MIT OR Apache-2.0, the revision Zed uses, so the types are the same as in the Zed fork).

| Crate | What it holds |
| --- | --- |
| `aurix-debug-model` | State without I/O or UI: `reference` (addresses, the probe-rs `@core` suffix), `protocol` (probe-rs `liveWatch`, `rttWindowOpened`, the RTT events, `disassemble` with signed offsets), `disassembly` (the window around a program counter, read further on either side), `breakpoints` (instruction breakpoints, and the condition, hit count and log message of any breakpoint), `source_breakpoints` (source line breakpoints per file, with their conditions), `variables` (scopes and variables as a tree read on demand, expanded nodes kept across stops, changed values marked), `live_watch` (expressions read on every core), `memory` (a memory window read again and again through one core, changed bytes marked, base64; writes only to the RAM of the chip: the TC27x's or the TC36x's, `RamMap`), `rtt` (RTT lines per core and channel, the down channels and the `rtt write` command that sends a line to one), `orti` (an ORTI 2.1/2.2 file: the attribute declarations with their ENUM items, the OS objects; what it does not know is skipped with a warning; `OsView` turns the values read into a table per object kind, ENUM values by name, changed values marked), `path_expr` (the first check of the C expressions the probe-rs fork reads: paths, `&`, casts, operators, `?:`; no calls, assignments or strings), `sw_trace` (the software trace of `aurix_trace.h` records on a binary RTT channel: records split across reads, the 32-bit timer's rounds, ids named by a file, spans, intervals, dropped records), `gtm` (the TC27x GTM's MCS instances: where their channel registers and memory are, the memory layout of MCFG_CTRL, a channel's registers decoded, the MCS instructions disassembled, and the GTM view read through the fork's `readRanges` request), `cache` (the caches of the TC27x and TC36x cores from their CSFRs: sizes, on or bypassed, the segments cached, a hit and miss measurement with the performance counters, and whether memory shown may be in a write-back DCACHE) |
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
cargo run --example cache_view -- dap-client/examples/tc36x_demo_2cores.json TC36x
```

`rtt_tail` needs the RTT test program on CPU0 and CPU1 (`zed/aurix_tcf_demo/build_rtt.ps1`,
`-Core 1`, started with `start_in_flash`, RAM only); `live_watch` and `break_when` the flash
demo started with `start_in_flash`. `cache_view` prints each core's caches as app B's Cache tab
does and measures once, the hits and the benchmark counters (it writes `CCTRL` and puts it
back); on the TC36x the caches are bypassed out of reset, so measure with `build_flash.ps1
-Board tc36x -Caches`. They leave the target halted: reset it afterwards.

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
| `include <file> [NAME=value]...` | The lines of another script file in its place (relative to the file the line is in, at most 8 deep, never itself), with `${NAME}` the values given; read with the script, before anything runs. Its lines report as `file:line`. `${HERE}` is the folder of the file a line is in; the paths of commands stay relative to the script run |
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
| `write <address>[@core] <bytes>` | `writeMemory`, RAM only: of the chip the attach file names (TC27x, or TC36x with its 192 KiB DSPRs and its DLMUs, cached and not cached) |
| `rtt send <core> <channel> <text>` | A line to the core's RTT down channel |
| `rtt expect <core> <channel> "<regex>" [within <time>]` | A line on the up channel that matches, after the last one matched |
| `sleep <time>`, `disconnect` | The run ends with `disconnect` anyway |
| `trace expect [core] <id> count\|span <stat>\|interval <stat> <op> <value> [within <time>]` | The software trace (`aurix_trace.h` records on an RTT channel whose `dataFormat` is `BinaryLE`): the records of an id, its spans or the interval between its records, `<stat>` min, max, avg or last, times as `750ns`, `2.5us`, `10ms`, `1s`; the ids named in `<attach>.trace` (`id = name`) or by number. Waits until it holds (default 10 s) |
| `trace expect [core] <id> load <op> <percent>` | The share of the time in the id's spans since the core's first record or last drop (`30%`, nested spans count in each) |
| `trace expect [core] dropped <op> <count>`, `trace clear`, `trace save <file>` | The records the target dropped (its buffer full); forget what was read so far (the backlog of a target nobody read); the events kept as CSV (`time_ns,core,id,name,kind,arg,span_ns`) |
| `let <name> = <expression>` | A script variable |
| `let <name> = read <expression>` | `evaluate` in the top frame of the stopped core |
| `let <name> = read live <global>[@core]` | One `liveWatch` |
| `let <name> = read memory <address>[@core] 1\|2\|4\|8` | `readMemory`, an unsigned little-endian integer |
| `file write <file> [<text>]`, `file append <file> <text>` | A line to a file (relative to the script, in double quotes when the name has spaces): `write` makes the file anew (without text: empty), `append` adds to it. Before `attach` too |
| `file delete <file>` | Removes it (a file not there is no error) |
| `let <name> = file read\|lines\|exists <file>` | All of the file (without its last line end), the number of lines, or 1/0 |
| `let <name> = file line <file> <n>` | Line `n`, from 1 (`${k}` in a loop); a line past the end ends the run |
| `if <condition>` … [`else` …] `end` | A branch |
| `while <condition>` … `end`, `repeat <count>` … `end` | Loops |
| `check <condition>` | An expectation on script variables (it shows their values) |
| `fail [<message>]` | A failed expectation (after an `if`) |
| `print <text>` | Writes a line |

Script variables, PRACTICE's macros: expressions name them as they are (`steps + 1`,
`i < 3 && sum == 18`), with C's integers (`0x2A`, `0b101`, `'a'`), operators and precedence,
text in double quotes (`state == "Running"`) and decimals compared as numbers. Any other line
takes a variable's value with `${name}` when it runs (`expect memory ${base}@0 2A`,
`print i = ${i}`); a `--set` value is a variable's first value. A value read from the target is
kept as an integer when it is one (`0x0000002A`, `97 'a'`), else as its text; one that cannot be
read ends the run (exit 2).

Exit code 0: every expectation held; 1: one did not (the run stops there, or goes on with
`--keep-going` and lists them); 2: a mistake in the script, the server, a wait that ran out of
time, or Ctrl+C. `--junit` writes the script as a testsuite and each expectation that ran as a
testcase, in a loop each time (`#2`, `#3`, …), and a `fail` that ran too (expectations not reached
are skipped). Examples in `script/examples` (`lib/trace_start.aurix` is the start both trace scripts include): `sw_trace` (the ORTI demo built with `build_orti.ps1 -Trace`: the timer ISR every 10 ms and 2.8 µs long, the task slices 400 ms, nothing dropped), `sw_trace_2cores` (with `trace_cpu1` on CPU1, `build_trace_cpu1.ps1`, 2,200 records a second: both cores at once, and how full each RTT buffer got, `aurix_trace_high`), `loop_until` (flash demo: step over through the loop
of `accumulate()` with `while i < 3`, counting the steps, then `check i == 3 && steps == 11 &&
sum == 18`), `file_table` (flash demo: the sums before each pass of that loop from
`data/accumulate_sums.txt`, one a line, checked and logged as CSV to `${TEMP}`), `demo_flash`, `conditions`
and `data_breakpoint` (flash demo), `rtt_echo` (RTT test program on CPU0 and CPU1), and
`watch_after_break`, a data breakpoint next to an instruction breakpoint (it did not halt before
probe-rs fork `149db65`), and `orti_paths` (the ORTI demo `zed/aurix_tcf_demo/build_orti.ps1`,
RAM only): `expect` and `expect live` on paths such as `OsTcb[1].prio` and
`OsRunningTcb->state`, and a breakpoint for one task, `if OsRunningTcb->prio == 2` (probe-rs fork
`d8954dd`), `gtm_mcs` (the GTM test program `zed/aurix_tcf_demo/build_gtm.ps1`, RAM only: two MCS1 channels
run and their counters move), `orti_expressions` (the ORTI demo: casts, `*(unsigned*)&OsTicks`, pointer differences and an index read from the target, `OsTcb[OsCurrentTask].prio`, by `expect live`), and `step_over_loop` (flash demo): step over through a `for` loop whose condition
sits after the body must stop in the next iteration (before probe-rs fork `c5d7411` it ran to the
end of the loop), and `step_over_conditions` (flash demo): step over a call that holds a
breakpoint whose condition does not hold, or a log point (its line comes), goes on over the call;
one without a condition stops it in the call (before the probe-rs fork fixed it, the step stopped
there in every case and the log point printed nothing).

On the TC36x (TriBoard TC3X5) the same with `tc36x_demo_2cores.json` (the flash demo
`demo_flash_tc36x.elf`; CPU1 of the test board is in boot halt and reads as sleeping) and
`tc36x_rtt_cpu0.json` (`build_rtt.ps1 -Board tc36x`, CPU0 only). The scripts take the attach
file from `--set ATTACH=...`: the flash demo ones `tc36x_demo_2cores.json`, `orti_paths` and
`orti_expressions` `tc36x_orti_demo.json` (`build_orti.ps1 -Board tc36x`), `sw_trace`
`tc36x_orti_trace.json` (`build_orti.ps1 -Trace -Board tc36x`) and `sw_trace_2cores`
`tc36x_trace_2cores.json` (with `build_trace_cpu1.ps1 -Board tc36x`, started with
`start_in_flash --core 1`, which takes the boot-halted CPU1 out of boot halt). On the board they
hold as on the TC275 (STM0 runs at 100 MHz there too). So does `gtm_mcs` with
`tc36x_gtm_demo.json` (`build_gtm.ps1 -Board tc36x`): the boot application of the test board
leaves the GTM off (`GTM_CLC` = 3), so the program switches it on; and since a word of the TC3xx
MCS memory never written fails its ECC check when an MCS reads ahead into it, stopping the
channel, the program writes the whole memory of MCS1 first. `"suspendPeripherals": true` in a
TC36x attach file (probe-rs fork) stops the system timer of each core of the session while the
core is halted (STM0 with CPU0): an OS tick or a timeout does not run on behind a breakpoint.
The session puts the registers back when it ends.

```text
cargo test
cargo clippy --all-targets -- -D warnings
```
