# Build reset_demo.elf (TriCore, AURIX GCC): an image the reset starts, entry function core0_main.
# Code in the PF1 test sector 0x8021C000; PF0 needs the jump at 0x80000020 to reach it.
$ErrorActionPreference = 'Stop'
$tc = 'C:\Infineon\AURIX-Configuration-Studio-1.0.24\eclipse\tricore-gcc11\bin'
Push-Location $PSScriptRoot
try {
    New-Item -ItemType Directory -Force build | Out-Null
    & "$tc\tricore-elf-gcc.exe" -mcpu=tc27xx -c -o build\reset_start.o src\reset_start.S
    & "$tc\tricore-elf-gcc.exe" -mcpu=tc27xx -O0 -g3 -ffreestanding -nostdlib `
        -c -o build\reset_demo.o src\reset_demo.c
    & "$tc\tricore-elf-gcc.exe" -mcpu=tc27xx -nostdlib -T reset_demo.ld '-Wl,-Map=build\reset_demo.map' `
        -o build\reset_demo.elf build\reset_start.o build\reset_demo.o
    & "$tc\tricore-elf-objdump.exe" -d build\reset_demo.elf > build\reset_demo.dis
    Get-Item build\reset_demo.elf | Select-Object Name, Length, LastWriteTime
}
finally {
    Pop-Location
}
