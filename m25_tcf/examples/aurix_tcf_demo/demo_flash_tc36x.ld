/* aurix_tcf_demo, flash variant for the TC36x (TriBoard TC3X5): code in PF1
 * sector 0x80400000 (16 KB; PF1 0xA0400000..0xA04FFFFF is erased on the test
 * board), variables in the CPU0 DSPR.  Otherwise as demo_flash.ld: no
 * initialised data, no startup code; start_in_flash points CPU0 at main and
 * sets the stack (A10 = 0x7000C000). */
ENTRY(main)

MEMORY
{
  pflash (rx) : ORIGIN = 0x80400000, LENGTH = 16K
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
  ASSERT(SIZEOF(.data) == 0, "demo_flash_tc36x: initialised data is not copied from flash")
}
