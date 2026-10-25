# Build orti_demo.elf: the ORTI test program (RAM only, see src/orti_demo.c and orti_demo.ort).
# -Trace: the same with the software trace (src/aurix_trace.h, RTT channel "SwTrace"),
# build\orti_trace.elf; attach with tc275_orti_trace.json (rttEnabled, channel 0 BinaryLE).
# Start it with: start_in_flash.exe build\orti_demo.elf (or build\orti_trace.elf), which writes
# it to RAM and leaves CPU0 halted at main.
param([switch]$Trace)
$ErrorActionPreference = 'Stop'
$tc = @('C:\Infineon\AURIX-Configuration-Studio-1.0.24\eclipse\tricore-gcc11\bin',
         'C:\Infineon\AURIX-Studio-1.10.40\tools\Compilers\tricore-gcc11\bin') |
    Where-Object { Test-Path "$_\tricore-elf-gcc.exe" } | Select-Object -First 1
if (-not $tc) { throw 'AURIX GCC (tricore-gcc11) not found' }
$name = if ($Trace) { 'orti_trace' } else { 'orti_demo' }
$defines = @()
if ($Trace) { $defines += '-DSW_TRACE' }
Push-Location $PSScriptRoot
try {
    New-Item -ItemType Directory -Force build | Out-Null
    & "$tc\tricore-elf-gcc.exe" -mcpu=tc27xx -O0 -g3 -ffreestanding -nostdlib @defines `
        -c -o "build\$name.o" src\orti_demo.c
    if ($LASTEXITCODE) { throw "compile failed ($LASTEXITCODE)" }
    & "$tc\tricore-elf-gcc.exe" -mcpu=tc27xx -nostdlib -T orti_demo.ld "-Wl,-Map=build\$name.map" `
        -o "build\$name.elf" "build\$name.o"
    if ($LASTEXITCODE) { throw "link failed ($LASTEXITCODE)" }
    & "$tc\tricore-elf-objdump.exe" -d "build\$name.elf" > "build\$name.dis"
    & "$tc\tricore-elf-readelf.exe" -lW "build\$name.elf"
    Get-Item "build\$name.elf" | Select-Object Name, Length, LastWriteTime
}
finally {
    Pop-Location
}
