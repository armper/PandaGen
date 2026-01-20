OUTPUT_FORMAT(elf64-x86-64)
ENTRY(_start)

MEMORY
{
    RAM (rwx) : ORIGIN = 0xffffffff80000000, LENGTH = 16M
    ROM (rx)  : ORIGIN = 0x00100000, LENGTH = 16M
}

SECTIONS
{
    .text : ALIGN(4096)
    {
        KEEP(*(.text.entry))
        *(.text .text.*)
    } > RAM AT > ROM

    .rodata : ALIGN(4096)
    {
        *(.rodata .rodata.*)
    } > RAM AT > ROM

    .data : ALIGN(4096)
    {
        *(.data .data.*)
    } > RAM AT > ROM

    .bss : ALIGN(4096)
    {
        *(.bss .bss.*)
        *(.bss.stack)
        *(COMMON)
    } > RAM AT > ROM

    .limine_requests : ALIGN(16)
    {
        __limine_requests_start = .;
        KEEP(*(.limine_requests))
        __limine_requests_end = .;
    } > RAM AT > ROM

    /DISCARD/ :
    {
        *(.eh_frame .eh_frame_hdr)
    }
}
