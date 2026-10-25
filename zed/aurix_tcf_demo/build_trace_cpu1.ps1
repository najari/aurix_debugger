# Build trace_cpu1.elf: a software trace on CPU1 (RAM only, see src/trace_cpu1.c).
# Start it with: start_in_flash.exe build\trace_cpu1.elf --core 1, next to the ORTI demo's
# trace build on CPU0 (build_orti.ps1 -Trace), and attach with tc275_trace_2cores.json.
$ErrorActionPreference = 'Stop'
$tc = @('C:\Infineon\AURIX-Configuration-Studio-1.0.24\eclipse\tricore-gcc11\bin',
         'C:\Infineon\AURIX-Studio-1.10.40\tools\Compilers\tricore-gcc11\bin') |
    Where-Object { Test-Path "$_\tricore-elf-gcc.exe" } | Select-Object -First 1
if (-not $tc) { throw 'AURIX GCC (tricore-gcc11) not found' }
Push-Location $PSScriptRoot
try {
    New-Item -ItemType Directory -Force build | Out-Null
    & "$tc\tricore-elf-gcc.exe" -mcpu=tc27xx -O0 -g3 -ffreestanding -nostdlib `
        -c -o build\trace_cpu1.o src\trace_cpu1.c
    if ($LASTEXITCODE) { throw "compile failed ($LASTEXITCODE)" }
    & "$tc\tricore-elf-gcc.exe" -mcpu=tc27xx -nostdlib -T trace_cpu1.ld "-Wl,-Map=build\trace_cpu1.map" `
        -o build\trace_cpu1.elf build\trace_cpu1.o
    if ($LASTEXITCODE) { throw "link failed ($LASTEXITCODE)" }
    & "$tc\tricore-elf-objdump.exe" -d build\trace_cpu1.elf > build\trace_cpu1.dis
    & "$tc\tricore-elf-readelf.exe" -lW build\trace_cpu1.elf
    Get-Item build\trace_cpu1.elf | Select-Object Name, Length, LastWriteTime
}
finally {
    Pop-Location
}
