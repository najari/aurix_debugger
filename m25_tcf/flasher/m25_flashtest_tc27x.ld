/* TC27x self-test: PF1 logical sector 0xA021C000 (unused by the application) */
PHDRS { p1 PT_LOAD; p2 PT_LOAD; }
SECTIONS
{
  .blob1 0x8021C400 : { KEEP(*(.blob1)) } :p1
  .blob2 0x8021D010 : { KEEP(*(.blob2)) } :p2
  /DISCARD/ : { *(.comment) }
}
