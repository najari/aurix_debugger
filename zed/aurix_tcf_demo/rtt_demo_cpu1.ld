/* rtt_demo for CPU1: code in the CPU1 PSPR, data in the CPU1 DSPR (global addresses).
 * Nothing goes to flash: start_in_flash --core 1 writes both segments to RAM.  The stack
 * (A10, set by start_in_flash --core 1 to 0xD000C000, the CPU1 DSPR at 0x6000C000) grows
 * down above the data. */
ENTRY(main)

MEMORY
{
  pspr (rx) : ORIGIN = 0x60100000, LENGTH = 8K
  dspr (rw) : ORIGIN = 0x6000A000, LENGTH = 2K
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
