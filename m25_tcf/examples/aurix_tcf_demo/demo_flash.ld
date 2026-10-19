/* aurix_tcf_demo, flash variant: code in PF1 sector 0x8021C000 (16 KB, unused
 * by the application), variables in the CPU0 DSPR.  The program has no
 * initialised data (.data), so nothing needs copying from flash to RAM; .bss
 * is not cleared either (no startup code) - g_ticks / g_result start with
 * whatever the DSPR holds.  tools/flash_start.js points core0 at main and sets
 * the stack (A10 = 0x7000C000, grows down into 0x7000A800..0x7000BFFF). */
ENTRY(main)

MEMORY
{
  pflash (rx) : ORIGIN = 0x8021C000, LENGTH = 16K
  dspr   (rw) : ORIGIN = 0x7000A000, LENGTH = 2K
}

/* One PT_LOAD with the code only: no ELF/program headers in front of it, so
 * the flasher writes nothing outside the sector. */
PHDRS { text PT_LOAD; }

SECTIONS
{
  .text   : { *(.text*) }    > pflash :text
  .rodata : { *(.rodata*) }  > pflash :text
  .data   : { *(.data*) *(.sdata*) *(.zdata*) } > dspr :NONE
  .bss    : { *(.bss*) *(.sbss*) *(.zbss*) *(COMMON) } > dspr :NONE
  /DISCARD/ : { *(.comment) }
  ASSERT(SIZEOF(.data) == 0, "demo_flash: initialised data is not copied from flash")
}
