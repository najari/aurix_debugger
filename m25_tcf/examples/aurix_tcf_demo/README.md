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
| `.vscode/launch.json` | one set of configurations per board, named `TC275: ...` and `TC36x: ...`. TC275: aurix-tcf RAM and two PFLASH configurations; probe-rs PFLASH and RAM configurations for CPU0, one each for CPU1 and CPU2, and a compound that starts all three. TC36x: probe-rs PFLASH and RAM configurations for CPU0 |
| `src/reset_start.S`, `src/reset_demo.c`, `reset_demo.ld`, `build_reset.ps1` | the reset image with `core0_main` (see below; needs a PF0 change) |
| `tools/pf0_jump_hex.py` | HEX files that point the PF0 reset jump at the reset image, and put it back |
| `.vscode/tasks.json` | build tasks, M25 flasher tasks (`../../tools/m25_flash_task.ps1`), probe-rs flash and start tasks, per board |
| `build_flash.ps1`, `demo_flash.ld` | the same program linked for PF1 sector `0x8021C000` (TC275) |
| `build_flash.ps1 -Board tc36x`, `demo_flash_tc36x.ld` | the same program linked for PF1 sector `0x80400000` (TC36x) → `build/demo_flash_tc36x.elf` |
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
| `TC275: aurix-tcf PFLASH CPU0` | the extension's Aurix Flasher (`enableFlashing: true`) | needs the TC27x C-step ED device entry (below); `preLaunchTask` only builds |
| `TC275: aurix-tcf PFLASH CPU0 (M25 flasher)` | M25 flasher (`tools/m25_flash_task.ps1`, `preLaunchTask`) | backs up the sector first, skips the write when flash already matches, resets and runs the target |

After the session starts:

1. `node tools/flash_start.js`: checks that flash holds the demo, suspends
   core0 and sets PC = `main`, A10 = `0x7000C000`, A11 = PCXI = 0.
2. Set breakpoints (`square`, ...) and continue; step over / into / out and
   the call stack work as in the RAM demo.
3. When done: stop the session and reset the board (the application in flash
   was never touched; core0's context was abandoned by `flash_start.js`).

Tasks: "TC275: m25 flash PF1 test image" / "TC275: m25 restore PF1 test sector"
write and undo the flasher self-test image (`../../flasher/m25_flashtest_tc27x.elf`,
same sector); "TC275: m25 flash demo (PF1 0x8021C000)" writes the demo with the M25
flasher.  Sector backups go to `.m25_flash_backup` (not tracked).

## Debugging with probe-rs (TAS, multi-core)

The probe-rs configurations use the probe-rs TriCore port
([najari/probe-rs `tricore`](https://github.com/najari/probe-rs/tree/tricore)),
built at `D:\work\claude\probe-rs` (`cargo build --release -p probe-rs-tools` and
`cargo build --release -p probe-rs-infineon --example start_in_flash`; change the
`runtimeExecutable` and task paths for another location), and the probe-rs
extension for VS Code (`probe-rs.probe-rs-debugger`). They reach the board
through the TAS server (probe `058b:0000`).

The boards are connected one at a time to the same TAS server, so every
configuration uses the first TAS target (`058b:0000`). Pick the configurations
of the board that is connected: a session of the other board's chip stops at
once (`the target is TC275, but the connected device (JTAG ID 0x10209083) is a
TC36x: use the TC36x configuration`).

| Configuration | What it does |
|---|---|
| `TC275: probe-rs PFLASH CPU0` | `preLaunchTask` writes `demo_flash.elf` with probe-rs (PF1 `0x8021C000` only, skipped when it is already there) and halts CPU0 at `main`; then attaches to CPU0 |
| `TC275: probe-rs RAM CPU0` | `preLaunchTask` builds `demo.elf`, writes it into the CPU0 PSPR and DSPR (no flash write) and halts CPU0 at `main`; then attaches to CPU0. Breakpoints beyond the 8 trigger units are software breakpoints there, so there is no limit of 8 |
| `TC275: probe-rs CPU1`, `... CPU2` | attach to CPU1 / CPU2, which keep running the application |
| `TC275: probe-rs CPU0 + CPU1 + CPU2` | compound: the three sessions side by side in the call stack view |
| `TC36x: probe-rs PFLASH CPU0` | `preLaunchTask` writes `demo_flash_tc36x.elf` with probe-rs (PF1 `0x80400000` only, skipped when it is already there) and halts CPU0 at `main`; then attaches to CPU0 |
| `TC36x: probe-rs RAM CPU0` | the same `demo.elf` as on the TC275 (its PSPR and DSPR addresses exist on the TC36x CPU0), written into RAM, CPU0 halted at `main` |

On the TC36x (TriBoard TC3X5; JTAG ID `0x10209083`, detected as `TC36x`; 2
cores, DSPR 192 KiB and PSPR 32 KiB each, PFLASH PF0 `0x80000000` and PF1
`0x80300000`, 2 MiB each, 16 KiB sectors):

- probe-rs writes the PFLASH with the TC3xx build of the flash loader
  (`probe-rs-infineon/loader/m25_flash_loader_tc3xx.bin`). The demo sector
  `0x80400000` is in the part of PF1 (`0xA0400000..0xA04FFFFF`) that was erased
  on the test board; `pflash_usage` (probe-rs-infineon example) shows which
  sectors hold data.
- CPU1 stays in boot halt (`SYSCON.BHALT`) until the application starts it. A
  halt of such a core fails with `Core 1 is in boot halt`, so the TC36x
  configurations do not set `allStop` and have no CPU1 configuration; flashing
  leaves such a core alone.
- The peripheral view uses the bundled `TC36XPD` SVD file.

The probe-rs debugger handles one core per session, so each core has its own
session. The configurations set `allStop`: a breakpoint, step or pause in one
session halts the other cores as well (stop reason `external`), and continuing
one runs them again. A core halted at its own breakpoint stays there until its
own session continues it. Notes:

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
  whole device. `probe-rs-infineon`'s `bus_read` example reads memory without
  touching the cores.
- Each breakpoint takes one of the 8 hardware trigger units of the core. In
  the PSPR of the core (the RAM demo) the breakpoints beyond the free units
  are `DEBUG` instructions written over the code; a breakpoint elsewhere
  (PFLASH) that finds no free unit takes one from a PSPR breakpoint, which
  becomes a `DEBUG` instruction. Trigger unit breakpoints stay through a reset
  and code the program copies into the PSPR; `DEBUG` instructions are planted
  again after it. Reads (memory view, disassembly) show the program, not the
  `DEBUG` instructions. If a session ends without a disconnect (the debugger
  is killed), its `DEBUG` instructions stay in RAM until the next session on
  that core of the same board (on this PC; the record is kept per TAS target,
  so a session on the other board does not touch it): it puts the code back
  from the record the killed session kept, and says so in the debug console.
- Flash backups: with the environment variable
  `PROBE_RS_TRICORE_FLASH_BACKUP` set to a folder (in VS Code:
  `"env": { "PROBE_RS_TRICORE_FLASH_BACKUP": "C:\\flash_backups" }` in the
  configuration), every download saves each PFLASH sector it erases, as it
  was, in `<folder>/<UTC time>/sector_<address>.json`. A sector that already
  holds the image is not erased and not saved. To put sectors back:
  `python tools/backup_to_hex.py <backup folder> restore.hex`, then
  `probe-rs download --chip TC275 --binary-format hex restore.hex`
  (`--chip TC36x` on the TC36x).
  `probe-rs-infineon`'s `pflash_check` example compares the flash with a
  backup.
- Another TAS server: set `probe` to `058b:0000:<host>[:<port>]/<target>`,
  for example `058b:0000:192.168.1.10:24817/DAS JTAG AURDUINO 46YPEADW`
  (`058b:0000:<host>/` takes its first target).
- Data breakpoints: right-click a variable (in the variables or the watch
  view, while the core is halted), **Break on Value Change** (or Break on
  Value Read / Access). Each takes trigger units shared with the
  breakpoints: one for a single byte, an even/odd pair for more.
  The core stops after the instruction that accessed the variable (stop
  reason `data breakpoint`). Conditions are refused. GDB's `watch`, `rwatch`
  and `awatch` work the same way.
- Options of a core configuration (`coreConfigs[]`):
  - `"stepInterrupts": "maskInstruction"` (the default) keeps interrupts
    off while an instruction is stepped (`ICR.IE` restored afterwards; an
    instruction that changes `IE` itself is stepped as it is); `"allow"`
    lets a pending interrupt be taken during the step.
  - `"perfWindow": { "start": "square", "stop": "0x8021c0a0" }` counts only
    between two functions or addresses (two free trigger units). The
    counters `ccnt`, `icnt`, `m1cnt`–`m3cnt` and `cctrl` are in the
    registers view under **Performance** (read-only there).
  - `"resetBehavior": "restore"` (the default) puts breakpoints, data
    breakpoints and the halt group back after a reset started by the
    application (watchdog, software reset) and lets it run; `"halt"` halts
    the core after the reset. The reset and its cause (`SCU_RSTSTAT`) are
    printed in the debug console either way.

### Reset image with `core0_main` (writes PF0)

`build_reset.ps1` builds `build/reset_demo.elf`: the demo call chain started by
the reset, with `core0_main` as the entry function. `src/reset_start.S` jumps
from the non-cached to the cached alias, sets PSW, the stack, A11 = PCXI = 0 and
the CSA list (64 contexts at `0x70010000`); `_c_start` (`src/reset_demo.c`)
disables the Safety and CPU0 watchdogs, clears `.bss` and calls `core0_main`.
The code lives in the PF1 test sector `0x8021C000` (`reset_demo.ld`); only
CPU0 runs, CPU1 and CPU2 stay halted after the reset.

The reset reaches it only when the jump at `0x80000020` in PF0 (after the boot
mode header; `ja 0x8003CD9A` into the application) is changed to
`j 0x8021C000`. This rewrites PF0 sector 0 and stops the application from
starting, so do it only with a sector backup and put the sector back after:

1. `pflash_check` (probe-rs-infineon example) against the backup: all sectors
   the same.
2. `python tools/pf0_jump_hex.py <backup>/sector_a0000000.json <dir>` writes
   `pf0_patch.hex` (the backup with the jump changed) and `pf0_restore.hex`
   (the backup), programmed pages only, so erased pages stay erased and the
   boot mode header is kept.
3. `probe-rs download --verify build\reset_demo.elf`, then
   `probe-rs download --binary-format hex --verify <dir>\pf0_patch.hex`.
4. Debug with a probe-rs `launch` and `"entryFunction": "core0_main"` on core
   0 (`flashingEnabled: false`): the reset runs to `core0_main`.
5. `probe-rs download --binary-format hex --verify <dir>\pf0_restore.hex`,
   `pflash_check` again (all the same), then write `demo_flash.elf` back into
   PF1 and reset.

Checked on 2026-10-23: launch and restart stop at `core0_main` (reason
`entry`, `g_ticks` = 0), a breakpoint in `square` shows the call chain down
to `_c_start`, the core runs without a watchdog reset; PF0 compared equal to
the backup after the restore.

### Aurix Flasher on a TC275 C-step ED (ShieldBuddy)

The flasher bundled with the extension lists TC27x D-step only; on this board
(JTAG ID `0x401DA083`) it stops with `No device found with JTAG ID
0x401DA083`.  See `m25_tcf/M25_4B_FLASHER.md`, section 14, for the device
entry to add and for removing the extension's automatic `-erase all` retry.

Keep run times short: the application's watchdog is not serviced while the
demo runs.

