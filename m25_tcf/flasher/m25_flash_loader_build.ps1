# Build the M25-4b flash loader (TriCore, AURIX GCC) for both devices:
#   m25_flash_loader.bin        TC37x (TC3xx DMU)
#   m25_flash_loader_tc27x.bin  TC27x (TC2xx FLASH0)
$ErrorActionPreference = 'Stop'
$tc = 'C:\Infineon\AURIX-Configuration-Studio-1.0.24\eclipse\tricore-gcc11\bin'
Push-Location $PSScriptRoot
try {
    foreach ($v in @(@{ name = 'm25_flash_loader';       flags = @() },
                     @{ name = 'm25_flash_loader_tc27x'; flags = @('-mcpu=tc27xx', '-DTC27X') })) {
        $n = $v.name
        & "$tc\tricore-elf-gcc.exe" -c -O2 -nostdlib -ffreestanding @($v.flags) -o "$n.o" m25_flash_loader.c
        if ($LASTEXITCODE) { throw "compile $n failed" }
        & "$tc\tricore-elf-gcc.exe" -nostdlib @($v.flags) -T m25_flash_loader.ld -o "$n.elf" "$n.o"
        if ($LASTEXITCODE) { throw "link $n failed" }
        & "$tc\tricore-elf-objcopy.exe" -O binary "$n.elf" "$n.bin"
        if ($LASTEXITCODE) { throw "objcopy $n failed" }
        Get-Item "$n.bin" | Select-Object Name, Length, LastWriteTime
    }
}
finally {
    Pop-Location
}
