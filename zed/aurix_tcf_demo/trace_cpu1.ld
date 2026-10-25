/* trace_cpu1: code in the CPU1 PSPR, data in the CPU1 DSPR (global addresses). Nothing goes
 * to flash: start_in_flash --core 1 writes both segments to RAM. The data is where rtt_demo_cpu1's
 * is, 6K for the 4 KiB trace buffer, up to 0x6000B800; the stack (A10 = 0xD000C000, the CPU1 DSPR
 * at 0x6000C000) grows down from above it. Lower in the DSPR are the context save areas a flash
 * program set up before (LCX 0xD0004080 on this board): writing there ends in a context trap. */
ENTRY(main)

MEMORY
{
  pspr (rx) : ORIGIN = 0x60100000, LENGTH = 8K
  dspr (rw) : ORIGIN = 0x6000A000, LENGTH = 6K
}

PHDRS
{
  text PT_LOAD;
  data PT_LOAD;
}

SECTIONS
{
  .text   : { *(.text*) }                     > pspr :text
  .rodata : { *(.rodata*) }                   > pspr :text
  .data   : { *(.data*) *(.sdata*) *(.zdata*) } > dspr :data
  .bss    : { *(.bss*) *(.sbss*) *(.zbss*) *(COMMON) } > dspr :data
}
