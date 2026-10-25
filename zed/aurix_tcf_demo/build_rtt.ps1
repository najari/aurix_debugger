# Build rtt_demo.elf: an RTT output test (CPU0 PSPR/DSPR, RAM only, see src/rtt_demo.c).
# Start it with: start_in_flash.exe build\rtt_demo.elf (writes it to RAM, CPU0 halted at main),
# then attach with rttEnabled (the Zed scenario "TC275: probe-rs RTT RAM CPU0").
$ErrorActionPreference = 'Stop'
$tc = @('C:\Infineon\AURIX-Configuration-Studio-1.0.24\eclipse\tricore-gcc11\bin',
         'C:\Infineon\AURIX-Studio-1.10.40\tools\Compilers\tricore-gcc11\bin') |
    Where-Object { Test-Path "$_\tricore-elf-gcc.exe" } | Select-Object -First 1
if (-not $tc) { throw 'AURIX GCC (tricore-gcc11) not found' }
Push-Location $PSScriptRoot
try {
    New-Item -ItemType Directory -Force build | Out-Null
    & "$tc\tricore-elf-gcc.exe" -mcpu=tc27xx -O0 -g3 -ffreestanding -nostdlib `
        -c -o build\rtt_demo.o src\rtt_demo.c
    if ($LASTEXITCODE) { throw "compile failed ($LASTEXITCODE)" }
    & "$tc\tricore-elf-gcc.exe" -mcpu=tc27xx -nostdlib -T rtt_demo.ld '-Wl,-Map=build\rtt_demo.map' `
        -o build\rtt_demo.elf build\rtt_demo.o
    if ($LASTEXITCODE) { throw "link failed ($LASTEXITCODE)" }
    & "$tc\tricore-elf-objdump.exe" -d build\rtt_demo.elf > build\rtt_demo.dis
    & "$tc\tricore-elf-readelf.exe" -lW build\rtt_demo.elf
    Get-Item build\rtt_demo.elf | Select-Object Name, Length, LastWriteTime
}
finally {
    Pop-Location
}
