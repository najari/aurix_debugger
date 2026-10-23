# Build demo.elf (TriCore, AURIX GCC from the AURIX Configuration Studio).
# -O0 -g: every function keeps its own frame and full debug information.
$ErrorActionPreference = 'Stop'
$tc = @('C:\Infineon\AURIX-Configuration-Studio-1.0.24\eclipse\tricore-gcc11\bin',
         'C:\Infineon\AURIX-Studio-1.10.40\tools\Compilers\tricore-gcc11\bin') |
    Where-Object { Test-Path "$_\tricore-elf-gcc.exe" } | Select-Object -First 1
if (-not $tc) { throw 'AURIX GCC (tricore-gcc11) not found' }
Push-Location $PSScriptRoot
try {
    New-Item -ItemType Directory -Force build | Out-Null
    & "$tc\tricore-elf-gcc.exe" -mcpu=tc27xx -O0 -g3 -ffreestanding -nostdlib `
        -c -o build\demo.o src\demo.c
    & "$tc\tricore-elf-gcc.exe" -mcpu=tc27xx -nostdlib -T demo.ld '-Wl,-Map=build\demo.map' `
        -o build\demo.elf build\demo.o
    & "$tc\tricore-elf-objdump.exe" -d build\demo.elf > build\demo.dis
    Get-Item build\demo.elf | Select-Object Name, Length, LastWriteTime
}
finally {
    Pop-Location
}
