# aurix-debugger (B)

The standalone debugger: a GPUI window on the probe-rs DAP server.

| Part | What it does |
| --- | --- |
| Toolbar | Continue, Pause, Step Over (line), Step Instruction, Step Into Instruction of the selected thread; the connection state, the attach file, Disconnect |
| Attach page (no session) | The attach files used before, newest first, each with its chip and the program of each core (`%APPDATA%\aurix-debugger\recent.txt`); "Open attach file…" (a file dialog); the server address; why the last attach failed. A click attaches. Attaching the same file again sends its breakpoints again; another file starts without them and with its own Live Watch expressions |
| Threads | One thread per core, with its state (running, stopped and why); a click selects it |
| Call stack | The frames of the selected thread; a click shows the frame's source, disassembly and variables |
| Source | The frame's source file (read from the path in the debug information) with its line marked (▶), back to it with PC, Reload after an edit; a click left of a line sets or removes a breakpoint on it (● set, ○ refused). A right click there opens the breakpoint's edit row: a condition (`v == 3 && k != 0`), a hit count (`3`, `>= 3`, `% 4`) and a log message (`v = {v}`, the core does not stop); Enter or Set sends them, Remove removes the breakpoint. ◆ marks a breakpoint with a condition or hit count, blue ◆ a log point, ◇ one the server refused (the reason shows in the edit row and the console). probe-rs checks them at each halt and runs the core on when they are not met |
| Disassembly | The instructions around the frame's program counter (▶), read further with ▲/▼, back to the PC; a click left of an instruction sets or removes a breakpoint on it (● set, ○ refused); ◆ marks where a source line's breakpoint was set |
| Variables | The frame's scopes (Static, Peripherals, Registers, Variables) as a tree; a click expands or collapses a node, what is expanded stays so at the next stop, and a value that differs from the previous stop's is red (locals and registers: same function on the same core; statics and peripherals: same core) |
| Console (tab) | The server's messages and stop descriptions (the same message twice within 500 ms shows once), and the lines of log points (each one) |
| RTT (a tab per channel) | The lines of each RTT up channel of every core, following the newest; Clear. When the core has a down channel, a line typed below (Enter) is sent to it (`rtt write`, probe-rs adds the line break) and shown as `» line` |
| Memory (tab) | 256 bytes from an address typed in (hex, `0x` optional, `@core` optional) or from a variable's "mem", read every 500 ms while the tab is shown, the cores running or not; through the core chosen, so `0xD000A000` reads that core's scratch-pad RAM; ◀/▶ move by 256 bytes; a byte that changed is red until the next change; the address asked for is marked. A click on a byte selects it; hex bytes typed into "Write at" (Enter) are written there through the same core, if they are all in RAM of the TC27x (scratch-pad RAMs, a core's local addresses, LMU); flash, the UCBs, peripheral registers and anything else are refused before anything is sent |
| Live Watch (tab) | Expressions (global variables) read every 500 ms on every core while the cores run; a value that changed since the last read is red; typed into the input (Enter), or sent from a Static variable with "watch"; × removes one; reading can be paused. The expressions are kept next to the attach file (`<attach>.watch`) |
| OS (tab) | The OS objects of an ORTI file (2.1, 2.2): a table per object kind (OS, tasks, ISRs, …) with a column per attribute, read every 500 ms while the tab is shown, the cores running or not (probe-rs `liveWatch` reads paths such as `OsTcb[1].prio` and `OsRunningTcb->state`); ENUM values by name, a value that changed since the read before is red, an expression the server cannot read (a cast, arithmetic) shows why in its own cell. "ORTI file…" chooses the file; its path is kept next to the attach file (`<attach>.orti`, relative to its folder). Without one, or with one that cannot be read (the reason shows in the tab and the console), debugging is as before. Try it with `tc275_orti_demo.json` and the RAM program of `zed/aurix_tcf_demo/build_orti.ps1` |
| GTM (tab) | The eight channels of one MCS instance of the GTM (MCS0–MCS3 to choose): enabled, PC, R0–R7, the flags N V Z C, the stack depth, IRQ and ERR, read every 500 ms while the tab is shown, the cores running or not; a value that changed since the read before is red. A click on a channel shows the MCS code around its PC, disassembled (▶ at the PC). The memory layout (MCFG_CTRL) gives each instance's memory size. Only read: nothing is written to the GTM. The registers are read word by word through the probe-rs fork's `readRanges` request. Known for the TC27x GTM; other chips say so. Try it with `tc275_gtm_demo.json` and the RAM program of `zed/aurix_tcf_demo/build_gtm.ps1` (two MCS1 channels) |

The splitters between the panels (threads | center | variables, source / disassembly, the
bottom panel) are dragged to resize them and double-clicked to put them back; the sizes are kept
in `%APPDATA%\aurix-debugger\layout.json`, and a smaller window shrinks the panels so that the
center keeps its least.

The state and rules of the views are the shared crates' (`../crates`: `aurix-debug-model`,
`aurix-dap-client`); this crate only draws them and sends the requests.

## Run

```text
probe-rs dap-server --port 50000
start_in_flash.exe C:\work\aurix_tcf_demo\build\demo_flash.elf
cargo run -- ..\crates\dap-client\examples\tc275_demo_3cores.json
```

Both arguments are optional: without an attach file the window opens on the attach page; the
second argument names another server (default `127.0.0.1:50000`). Any attach arguments of the
probe-rs fork work, `rttEnabled` included (`tc275_rtt_cpu0_cpu1.json`), and `speed` (kHz): the
example attach files ask for 6000, the fastest clock of the ShieldBuddy's DAS adapter (the TAS
server's own is 400 kHz, about a third of the speed of steps and memory views).

## GPUI and licenses

GPUI comes from the Zed checkout next to this repository (`../../zed`, tag v1.25.3, path
dependencies); the patches and `Cargo.lock` are Zed's, so GPUI builds with the versions it was
released with. `cargo metadata` of this crate: 618 packages in the build graph, no GPL one; the
19 Zed crates among them (gpui, gpui_platform, gpui_windows, collections, sum_tree, …) are all
Apache-2.0; `r-efi` and `self_cell` are dual licensed with a permissive option. The license of
this crate and the shared crates is not decided yet (`publish = false`).
