/* reset_demo: code in the PF1 test sector 0x8021C000 (16 KB, unused by the application), the
 * start code first; variables, stack and CSAs in the CPU0 DSPR. The reset reaches _start
 * through the jump at 0x80000020 in PF0. .bss is cleared by _c_start; there is no initialised
 * data. */
ENTRY(_start)

MEMORY
{
  pf1  (rx) : ORIGIN = 0x8021C000, LENGTH = 16K
  dspr (rw) : ORIGIN = 0x7000A000, LENGTH = 2K
}

/* One PT_LOAD with the code only: the flasher writes nothing outside the sector. */
PHDRS { text PT_LOAD; }

__stack_top = 0x7000C000;
/* 64 contexts of 64 bytes; LCX leaves 3 in reserve. */
__csa_start = 0x70010000;
__csa_count = 64;
__csa_lcx = __csa_start + 64 * (__csa_count - 4);

SECTIONS
{
  .text   : { *(.text.start) *(.text*) } > pf1 :text
  .rodata : { *(.rodata*) } > pf1 :text
  .data   : { *(.data*) *(.sdata*) *(.zdata*) } > dspr :NONE
  .bss    : {
    . = ALIGN(4);
    __bss_start = .;
    *(.bss*) *(.sbss*) *(.zbss*) *(COMMON)
    . = ALIGN(4);
    __bss_end = .;
  } > dspr :NONE
  /DISCARD/ : { *(.comment) }
  ASSERT(SIZEOF(.data) == 0, "reset_demo: initialised data is not copied from flash")
}
