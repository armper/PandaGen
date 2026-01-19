OUTPUT_FORMAT(elf64-x86-64)
ENTRY(_start)

PHDRS
{
    text PT_LOAD FLAGS(5);
    data PT_LOAD FLAGS(6);
}

KERNEL_VMA = 0xffffffff80000000;
KERNEL_LMA = 0x00100000;

SECTIONS
{
    . = KERNEL_VMA;

    .text : ALIGN(4096) AT(KERNEL_LMA)
    {
        KEEP(*(.text.entry))
        *(.text .text.*)
    } :text

    .rodata : ALIGN(4096) AT(ADDR(.rodata) - KERNEL_VMA + KERNEL_LMA)
    {
        *(.rodata .rodata.*)
    } :text

    .data : ALIGN(4096) AT(ADDR(.data) - KERNEL_VMA + KERNEL_LMA)
    {
        *(.data .data.*)
    } :data

    .bss : ALIGN(4096) AT(ADDR(.bss) - KERNEL_VMA + KERNEL_LMA)
    {
        *(.bss .bss.*)
        *(.bss.stack)
        *(COMMON)
    } :data

    .limine_requests : ALIGN(16) AT(ADDR(.limine_requests) - KERNEL_VMA + KERNEL_LMA)
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
