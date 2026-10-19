# Build demo_flash.elf: the demo linked for PFLASH (PF1 sector 0x8021C000), so it
# can be written with the M25 flasher (preLaunchTask) and debugged with
# breakpoints / stepping in flash.  See README.md, "Debugging from flash".
$ErrorActionPreference = 'Stop'
$tc = 'C:\Infineon\AURIX-Configuration-Studio-1.0.24\eclipse\tricore-gcc11\bin'
Push-Location $PSScriptRoot
try {
    New-Item -ItemType Directory -Force build | Out-Null
    & "$tc\tricore-elf-gcc.exe" -mcpu=tc27xx -O0 -g3 -ffreestanding -nostdlib `
        -c -o build\demo_flash.o src\demo.c
    if ($LASTEXITCODE) { throw "compile failed ($LASTEXITCODE)" }
    & "$tc\tricore-elf-gcc.exe" -mcpu=tc27xx -nostdlib -T demo_flash.ld '-Wl,-Map=build\demo_flash.map' `
        -o build\demo_flash.elf build\demo_flash.o
    if ($LASTEXITCODE) { throw "link failed ($LASTEXITCODE)" }
    & "$tc\tricore-elf-objdump.exe" -d build\demo_flash.elf > build\demo_flash.dis
    & "$tc\tricore-elf-readelf.exe" -lW build\demo_flash.elf
    Get-Item build\demo_flash.elf | Select-Object Name, Length, LastWriteTime
}
finally {
    Pop-Location
}
