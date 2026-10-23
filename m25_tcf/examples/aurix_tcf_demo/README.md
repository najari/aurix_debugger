# aurix_tcf_demo — VS Code + AURIX TCF agent test project

A small TriCore program with a real call chain
(`main → compute → accumulate → scale → square`) and full debug information,
for testing the call stack, source lines and the disassembly view.

It runs **from RAM** on CPU0 (code in PSPR `0x70100000`, data in DSPR
`0x7000A000`, stack top `0x7000C000`). The flashed application is not touched:
`tools/ram_load.js` saves the core registers and the RAM windows first, and
`tools/ram_restore.js` puts everything back.

## Files

| Path | Purpose |
|---|---|
| `src/demo.c` | the test program |
| `demo.ld` | linker script (PSPR code / DSPR data) |
| `build.ps1` | build with the AURIX GCC (`-O0 -g3`) → `build/demo.elf` |
| `tools/ram_load.js` | save state, load `demo.elf` into RAM, PC = `main` |
| `tools/ram_restore.js` | restore the saved registers + RAM |
| `.vscode/launch.json` | aurix-tcf: RAM configuration (`build/demo.elf`) and two PFLASH configurations (`build/demo_flash.elf`); probe-rs: one configuration per core (CPU0–CPU2) and a compound that starts all three |
| `.vscode/tasks.json` | build task, M25 flasher tasks (`../../tools/m25_flash_task.ps1`), probe-rs flash and start tasks |
| `build_flash.ps1`, `demo_flash.ld` | the same program linked for PF1 sector `0x8021C000` |
| `tools/flash_start.js` | point core0 at `main` of the flashed demo |
| `agent/tcf-agent.exe` | the AURIX TCF agent (build it from `m25_tcf`, see its README; not tracked) - set `aurix-tcf.agentPath` to this file |

## Use

1. `./build.ps1`
2. Start debugging in VS Code (F5) — the extension starts the agent.
3. `node tools/ram_load.js` (core0 suspended at `main`)
4. Set a breakpoint (e.g. `square`), Continue — call stack, source and
   disassembly show the demo program.
5. When done: let core0 return to `main` (remove the breakpoints, set one at
   `0x701000b0` - the `j` after `call compute` - and continue), stop the
   session, then `node tools/ram_restore.js` and resume the cores.

`ram_restore.js` refuses to run while demo calls are still open (FCX differs
from the saved value): every CALL takes a CSA off the free list and only the
matching RET gives it back, so restoring in the middle of the call chain would
leave the application with a broken free list.  After a successful restore the
state file is renamed, and `ram_load.js` refuses to overwrite an existing one.

`ram_load.js` sets A11 = 0 and PCXI = 0, so `main` is the outermost frame of
the call stack (as after reset).

Keep run times short: the application's watchdog is not serviced while the
demo runs.

## Debugging from flash (breakpoints and stepping in PFLASH)

`build_flash.ps1` links the same `src/demo.c` for PF1 sector `0x8021C000`
(16 KB, unused by the application) -> `build/demo_flash.elf`
(`demo_flash.ld`: one PT_LOAD with the code only, variables in the DSPR,
no initialised data, no startup code).  A RAM-linked ELF (`demo.elf`) cannot
be debugged this way: both flashers write ELF segments at their own addresses,
so it would only be written to SRAM and breakpoints would not work.

Two launch configurations write the ELF before the session starts:

| Configuration | Flasher | Notes |
|---|---|---|
| `aurix_tcf_demo (PFLASH, CPU0)` | the extension's Aurix Flasher (`enableFlashing: true`) | needs the TC27x C-step ED device entry (below); `preLaunchTask` only builds |
| `aurix_tcf_demo (PFLASH, CPU0, M25 flasher)` | M25 flasher (`tools/m25_flash_task.ps1`, `preLaunchTask`) | backs up the sector first, skips the write when flash already matches, resets and runs the target |

After the session starts:

1. `node tools/flash_start.js`: checks that flash holds the demo, suspends
   core0 and sets PC = `main`, A10 = `0x7000C000`, A11 = PCXI = 0.
2. Set breakpoints (`square`, ...) and continue; step over / into / out and
   the call stack work as in the RAM demo.
3. When done: stop the session and reset the board (the application in flash
   was never touched; core0's context was abandoned by `flash_start.js`).

Tasks: "m25: flash PF1 test image" / "m25: restore PF1 test sector" write and
undo the flasher self-test image (`../../flasher/m25_flashtest_tc27x.elf`,
same sector); "m25: flash demo (PF1 0x8021C000)" writes the demo with the M25
flasher.  Sector backups go to `.m25_flash_backup` (not tracked).

## Debugging with probe-rs (TAS, multi-core)

The probe-rs configurations use the probe-rs TriCore port
([najari/probe-rs `tricore`](https://github.com/najari/probe-rs/tree/tricore)),
built at `C:\work\probe-rs` (`cargo build -p probe-rs-tools` and
`cargo build -p probe-rs-infineon --example start_in_flash`; change the
`runtimeExecutable` and task paths for another location), and the probe-rs
extension for VS Code (`probe-rs.probe-rs-debugger`). They reach the board
through the TAS server (probe `058b:0000`).

| Configuration | What it does |
|---|---|
| `aurix_tcf_demo (probe-rs, TAS, PFLASH, CPU0)` | `preLaunchTask` writes `demo_flash.elf` with probe-rs (PF1 `0x8021C000` only, skipped when it is already there) and halts CPU0 at `main`; then attaches to CPU0 |
| `aurix_tcf_demo (probe-rs, TAS, CPU1)`, `... CPU2` | attach to CPU1 / CPU2, which keep running the application |
| `aurix_tcf_demo (probe-rs, CPU0 + CPU1 + CPU2)` | compound: the three sessions side by side in the call stack view |

The probe-rs debugger handles one core per session, so each core has its own
session; pausing, stepping or a breakpoint in one session halts only its core.
Notes:

- The peripheral view (`Peripherals` in the variables view) comes from the
  Infineon SVD file that matches the chip (`TC27XDPD`), found automatically;
  set `svdFile` in a core configuration to use another file.
- The flash map comes from the AURIX Flasher device file that matches the JTAG
  ID and SCU_CHIPID (the TC27x C-step ED entry of the aurix-tcf extension),
  else from the built-in TC27x map.
- CPU1 and CPU2 run the application, for which there is no ELF: their call
  stacks show addresses only.
- Global variables can be watched while the cores run with the **Live Watch**
  view of the `probe-rs Live Watch` extension
  (`../../../vscode/probe-rs-live-watch`), for example `g_ticks` and
  `g_result` of the demo.
- A probe-rs command line tool (`probe-rs read`, `reset`, ...) run during a
  session resumes every halted core, and a reset from any session resets the
  whole device.

### Aurix Flasher on a TC275 C-step ED (ShieldBuddy)

The flasher bundled with the extension lists TC27x D-step only; on this board
(JTAG ID `0x401DA083`) it stops with `No device found with JTAG ID
0x401DA083`.  See `m25_tcf/M25_4B_FLASHER.md`, section 14, for the device
entry to add and for removing the extension's automatic `-erase all` retry.

Keep run times short: the application's watchdog is not serviced while the
demo runs.

