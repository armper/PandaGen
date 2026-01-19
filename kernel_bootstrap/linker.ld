OUTPUT_FORMAT(elf64-x86-64)
ENTRY(_start)

PHDRS
{
    text PT_LOAD FLAGS(5);
    data PT_LOAD FLAGS(6);
}

SECTIONS
{
    . = 0x00100000;

    .text : ALIGN(16)
    {
        KEEP(*(.text.entry))
        *(.text .text.*)
    } :text

    .rodata : ALIGN(16)
    {
        *(.rodata .rodata.*)
    } :text

    .data : ALIGN(16)
    {
        *(.data .data.*)
    } :data

    .bss : ALIGN(16)
    {
        *(.bss .bss.*)
        *(.bss.stack)
        *(COMMON)
    } :data

    .limine_requests : ALIGN(16)
    {
        __limine_requests_start = .;
        KEEP(*(.limine_requests))
        __limine_requests_end = .;
    } :data

    /DISCARD/ :
    {
        *(.eh_frame .eh_frame_hdr)
    }
}
