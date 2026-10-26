# Build gtm_demo.elf: the GTM MCS test program (RAM only, see src/gtm_demo.c and the MCS program in it).
# Start it with: start_in_flash.exe build\gtm_demo.elf, which writes it to RAM and leaves CPU0
# halted at main.
# -Board tc36x: the same for the TC3xx GTM (-DGTM_TC3XX: its MCS registers, and main switches the
# GTM on), build\gtm_demo_tc36x.elf.
param([ValidateSet('tc275', 'tc36x')][string]$Board = 'tc275')
$ErrorActionPreference = 'Stop'
$tc = @('C:\Infineon\AURIX-Configuration-Studio-1.0.24\eclipse\tricore-gcc11\bin',
         'C:\Infineon\AURIX-Studio-1.10.40\tools\Compilers\tricore-gcc11\bin') |
    Where-Object { Test-Path "$_\tricore-elf-gcc.exe" } | Select-Object -First 1
if (-not $tc) { throw 'AURIX GCC (tricore-gcc11) not found' }
$name = if ($Board -eq 'tc36x') { 'gtm_demo_tc36x' } else { 'gtm_demo' }
$defines = if ($Board -eq 'tc36x') { @('-DGTM_TC3XX') } else { @() }
Push-Location $PSScriptRoot
try {
    New-Item -ItemType Directory -Force build | Out-Null
    & "$tc\tricore-elf-gcc.exe" -mcpu=tc27xx -O0 -g3 -ffreestanding -nostdlib $defines `
        -c -o "build\$name.o" src\gtm_demo.c
    if ($LASTEXITCODE) { throw "compile failed ($LASTEXITCODE)" }
    & "$tc\tricore-elf-gcc.exe" -mcpu=tc27xx -nostdlib -T gtm_demo.ld "-Wl,-Map=build\$name.map" `
        -o "build\$name.elf" "build\$name.o"
    if ($LASTEXITCODE) { throw "link failed ($LASTEXITCODE)" }
    & "$tc\tricore-elf-objdump.exe" -d "build\$name.elf" > "build\$name.dis"
    & "$tc\tricore-elf-readelf.exe" -lW "build\$name.elf"
    Get-Item "build\$name.elf" | Select-Object Name, Length, LastWriteTime
}
finally {
    Pop-Location
}

