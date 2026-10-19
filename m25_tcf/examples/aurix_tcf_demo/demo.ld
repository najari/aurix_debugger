/* aurix_tcf_demo: code in the CPU0 PSPR, data in the CPU0 DSPR (bus aliases).
 * The stack (A10) is set by tools/ram_load.js to 0x7000C000 (grows down into
 * the DSPR window 0x7000A800..0x7000BFFF, above the data). */
ENTRY(main)

MEMORY
{
  pspr (rx) : ORIGIN = 0x70100000, LENGTH = 8K
  dspr (rw) : ORIGIN = 0x7000A000, LENGTH = 2K
}

SECTIONS
{
  .text   : { *(.text*) }                     > pspr
  .rodata : { *(.rodata*) }                   > pspr
  .data   : { *(.data*) *(.sdata*) *(.zdata*) } > dspr
  .bss    : { *(.bss*) *(.sbss*) *(.zbss*) *(COMMON) } > dspr
}
