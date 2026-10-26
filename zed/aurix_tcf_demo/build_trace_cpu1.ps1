# Build trace_cpu1.elf: a software trace on CPU1 (RAM only, see src/trace_cpu1.c).
# Start it with: start_in_flash.exe build\trace_cpu1.elf --core 1, next to the ORTI demo's
# trace build on CPU0 (build_orti.ps1 -Trace), and attach with tc275_trace_2cores.json.
# -Board tc36x: the same for the TC36x (STM0 at 0xF0001000), build\trace_cpu1_tc36x.elf; with
# build_orti.ps1 -Trace -Board tc36x and tc36x_trace_2cores.json.
param([ValidateSet('tc275', 'tc36x')][string]$Board = 'tc275')
$ErrorActionPreference = 'Stop'
$tc = @('C:\Infineon\AURIX-Configuration-Studio-1.0.24\eclipse\tricore-gcc11\bin',
         'C:\Infineon\AURIX-Studio-1.10.40\tools\Compilers\tricore-gcc11\bin') |
    Where-Object { Test-Path "$_\tricore-elf-gcc.exe" } | Select-Object -First 1
if (-not $tc) { throw 'AURIX GCC (tricore-gcc11) not found' }
$name = if ($Board -eq 'tc36x') { 'trace_cpu1_tc36x' } else { 'trace_cpu1' }
$stm = if ($Board -eq 'tc36x') { '0xF0001010u' } else { '0xF0000010u' }
Push-Location $PSScriptRoot
try {
    New-Item -ItemType Directory -Force build | Out-Null
    & "$tc\tricore-elf-gcc.exe" -mcpu=tc27xx -O0 -g3 -ffreestanding -nostdlib `
        "-DSTM0_TIM0_ADDRESS=$stm" "-DAURIX_TRACE_STM_ADDRESS=$stm" -c -o "build\$name.o" src\trace_cpu1.c
    if ($LASTEXITCODE) { throw "compile failed ($LASTEXITCODE)" }
    & "$tc\tricore-elf-gcc.exe" -mcpu=tc27xx -nostdlib -T trace_cpu1.ld "-Wl,-Map=build\$name.map" `
        -o "build\$name.elf" "build\$name.o"
    if ($LASTEXITCODE) { throw "link failed ($LASTEXITCODE)" }
    & "$tc\tricore-elf-objdump.exe" -d "build\$name.elf" > "build\$name.dis"
    & "$tc\tricore-elf-readelf.exe" -lW "build\$name.elf"
    Get-Item "build\$name.elf" | Select-Object Name, Length, LastWriteTime
}
finally {
    Pop-Location
}
