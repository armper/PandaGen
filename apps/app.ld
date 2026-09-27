/* PandaGen programs (PROC-004): linked at 0x400000, code, read-only data
 * and data in separate pages so each piece is writable or runnable,
 * never both. The first 4 MiB stay unmapped: a null pointer faults. */
ENTRY(_start)

PHDRS
{
    text PT_LOAD FLAGS(5);   /* read, execute */
    rodata PT_LOAD FLAGS(4); /* read */
    data PT_LOAD FLAGS(6);   /* read, write */
}

SECTIONS
{
    . = 0x400000;
    .text : { *(.text._start) *(.text .text.*) } :text

    . = ALIGN(4096);
    .rodata : { *(.rodata .rodata.*) } :rodata

    . = ALIGN(4096);
    .data : { *(.data .data.*) } :data
    .bss : { *(.bss .bss.*) *(COMMON) } :data

    /DISCARD/ : { *(.eh_frame*) *(.comment) *(.note*) }
}
