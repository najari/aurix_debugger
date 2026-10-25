# Build orti_demo.elf: the ORTI test program (RAM only, see src/orti_demo.c and orti_demo.ort).
# Start it with: start_in_flash.exe build\orti_demo.elf, which writes it to RAM and leaves CPU0
# halted at main.
$ErrorActionPreference = 'Stop'
$tc = @('C:\Infineon\AURIX-Configuration-Studio-1.0.24\eclipse\tricore-gcc11\bin',
         'C:\Infineon\AURIX-Studio-1.10.40\tools\Compilers\tricore-gcc11\bin') |
    Where-Object { Test-Path "$_\tricore-elf-gcc.exe" } | Select-Object -First 1
if (-not $tc) { throw 'AURIX GCC (tricore-gcc11) not found' }
Push-Location $PSScriptRoot
try {
    New-Item -ItemType Directory -Force build | Out-Null
    & "$tc\tricore-elf-gcc.exe" -mcpu=tc27xx -O0 -g3 -ffreestanding -nostdlib `
        -c -o build\orti_demo.o src\orti_demo.c
    if ($LASTEXITCODE) { throw "compile failed ($LASTEXITCODE)" }
    & "$tc\tricore-elf-gcc.exe" -mcpu=tc27xx -nostdlib -T orti_demo.ld "-Wl,-Map=build\orti_demo.map" `
        -o build\orti_demo.elf build\orti_demo.o
    if ($LASTEXITCODE) { throw "link failed ($LASTEXITCODE)" }
    & "$tc\tricore-elf-objdump.exe" -d build\orti_demo.elf > build\orti_demo.dis
    & "$tc\tricore-elf-readelf.exe" -lW build\orti_demo.elf
    Get-Item build\orti_demo.elf | Select-Object Name, Length, LastWriteTime
}
finally {
    Pop-Location
}
