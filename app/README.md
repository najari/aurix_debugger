# aurix-debugger (B)

The standalone debugger: a GPUI window on the probe-rs DAP server.

| Part | What it does |
| --- | --- |
| Toolbar | Continue, Pause, Step Over (line), Step Instruction, Step Into Instruction of the selected thread; the connection state, the attach file, Disconnect |
| Attach page (no session) | The attach files used before, newest first, each with its chip and the program of each core (`%APPDATA%\aurix-debugger\recent.txt`); "Open attach file…" (a file dialog); the server address; why the last attach failed. A click attaches. Attaching the same file again sends its breakpoints again; another file starts without them and with its own Live Watch expressions |
| Threads | One thread per core, with its state (running, stopped and why); a click selects it |
| Call stack | The frames of the selected thread; a click shows the frame's source, disassembly and variables |
| Source | The frame's source file (read from the path in the debug information) with its line marked (▶), back to it with PC, Reload after an edit; a click left of a line sets or removes a breakpoint on it (● set, ○ refused) |
| Disassembly | The instructions around the frame's program counter (▶), read further with ▲/▼, back to the PC; a click left of an instruction sets or removes a breakpoint on it (● set, ○ refused); ◆ marks where a source line's breakpoint was set |
| Variables | The frame's scopes (Static, Peripherals, Registers, Variables) as a tree; a click expands or collapses a node, what is expanded stays so at the next stop, and a value that differs from the previous stop's is red (locals and registers: same function on the same core; statics and peripherals: same core) |
| Console (tab) | The server's messages and stop descriptions |
| RTT (a tab per channel) | The lines of each RTT up channel of every core, following the newest; Clear |
| Memory (tab) | 256 bytes from an address typed in (hex, `0x` optional, `@core` optional) or from a variable's "mem", read every 500 ms while the tab is shown, the cores running or not; through the core chosen, so `0xD000A000` reads that core's scratch-pad RAM; ◀/▶ move by 256 bytes; a byte that changed is red until the next change; the address asked for is marked |
| Live Watch (tab) | Expressions (global variables) read every 500 ms on every core while the cores run; a value that changed since the last read is red; typed into the input (Enter), or sent from a Static variable with "watch"; × removes one; reading can be paused. The expressions are kept next to the attach file (`<attach>.watch`) |

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
probe-rs fork work, `rttEnabled` included (`tc275_rtt_cpu0_cpu1.json`).

## GPUI and licenses

GPUI comes from the Zed checkout next to this repository (`../../zed`, tag v1.25.3, path
dependencies); the patches and `Cargo.lock` are Zed's, so GPUI builds with the versions it was
released with. `cargo metadata` of this crate: 618 packages in the build graph, no GPL one; the
19 Zed crates among them (gpui, gpui_platform, gpui_windows, collections, sum_tree, …) are all
Apache-2.0; `r-efi` and `self_cell` are dual licensed with a permissive option. The license of
this crate and the shared crates is not decided yet (`publish = false`).
