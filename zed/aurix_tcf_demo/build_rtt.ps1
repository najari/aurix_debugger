# Build rtt_demo.elf: an RTT output test (RAM only, see src/rtt_demo.c).
# -Core 0 (default): CPU0 PSPR/DSPR, build\rtt_demo.elf.
# -Core 1: CPU1 PSPR/DSPR, build\rtt_demo_cpu1.elf.
# Start it with: start_in_flash.exe build\rtt_demo.elf (or build\rtt_demo_cpu1.elf --core 1),
# which writes it to RAM and leaves the core halted at main, then attach with rttEnabled
# (the Zed scenarios "TC275: probe-rs RTT RAM CPU0" and "... CPU0 + CPU1").
param([ValidateSet(0, 1)][int]$Core = 0)
$ErrorActionPreference = 'Stop'
$tc = @('C:\Infineon\AURIX-Configuration-Studio-1.0.24\eclipse\tricore-gcc11\bin',
         'C:\Infineon\AURIX-Studio-1.10.40\tools\Compilers\tricore-gcc11\bin') |
    Where-Object { Test-Path "$_\tricore-elf-gcc.exe" } | Select-Object -First 1
if (-not $tc) { throw 'AURIX GCC (tricore-gcc11) not found' }
$name = if ($Core -eq 1) { 'rtt_demo_cpu1' } else { 'rtt_demo' }
Push-Location $PSScriptRoot
try {
    New-Item -ItemType Directory -Force build | Out-Null
    & "$tc\tricore-elf-gcc.exe" -mcpu=tc27xx -O0 -g3 -ffreestanding -nostdlib `
        "-DCORE_NAME=`"CPU$Core`"" -c -o "build\$name.o" src\rtt_demo.c
    if ($LASTEXITCODE) { throw "compile failed ($LASTEXITCODE)" }
    & "$tc\tricore-elf-gcc.exe" -mcpu=tc27xx -nostdlib -T "$name.ld" "-Wl,-Map=build\$name.map" `
        -o "build\$name.elf" "build\$name.o"
    if ($LASTEXITCODE) { throw "link failed ($LASTEXITCODE)" }
    & "$tc\tricore-elf-objdump.exe" -d "build\$name.elf" > "build\$name.dis"
    & "$tc\tricore-elf-readelf.exe" -lW "build\$name.elf"
    Get-Item "build\$name.elf" | Select-Object Name, Length, LastWriteTime
}
finally {
    Pop-Location
}
