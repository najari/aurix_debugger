/* orti_demo: as rtt_demo.ld, code in the CPU0 PSPR, data in the CPU0 DSPR (bus aliases).
 * Nothing goes to flash: start_in_flash writes both segments to RAM. The stack (A10, set by
 * start_in_flash to 0x7000C000) grows down above the data. */
ENTRY(main)

MEMORY
{
  pspr (rx) : ORIGIN = 0x70100000, LENGTH = 8K
  dspr (rw) : ORIGIN = 0x7000A000, LENGTH = 2K
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
