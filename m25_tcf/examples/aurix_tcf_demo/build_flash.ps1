# Build demo_flash.elf: the demo linked for PFLASH (PF1 sector 0x8021C000), so it
# can be written with the M25 flasher (preLaunchTask) and debugged with
# breakpoints / stepping in flash.  See README.md, "Debugging from flash".
# -Board tc36x builds demo_flash_tc36x.elf for the TC36x (PF1 sector 0x80400000).
param([ValidateSet('tc275', 'tc36x')][string]$Board = 'tc275')
$ErrorActionPreference = 'Stop'
$name = if ($Board -eq 'tc36x') { 'demo_flash_tc36x' } else { 'demo_flash' }
$tc = @('C:\Infineon\AURIX-Configuration-Studio-1.0.24\eclipse\tricore-gcc11\bin',
         'C:\Infineon\AURIX-Studio-1.10.40\tools\Compilers\tricore-gcc11\bin') |
    Where-Object { Test-Path "$_\tricore-elf-gcc.exe" } | Select-Object -First 1
if (-not $tc) { throw 'AURIX GCC (tricore-gcc11) not found' }
Push-Location $PSScriptRoot
try {
    New-Item -ItemType Directory -Force build | Out-Null
    & "$tc\tricore-elf-gcc.exe" -mcpu=tc27xx -O0 -g3 -ffreestanding -nostdlib `
        -c -o "build\$name.o" src\demo.c
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
