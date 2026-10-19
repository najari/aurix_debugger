# m25_4b_flash_selftest.ps1 - M25-4b flash self-test runbook
#
# Runs the full ELF flash cycle against the live target through the TCF agent:
#   backup(sector) -> save -> load(loader) -> flash(ELF) -> flashrestore(backup)
#   -> verifysector(backup) -> stop
# Each step must exit 0.  The flash step verifies the written sector itself,
# verifysector proves the sector is back to its original pages (data and
# erased pages alike).
#
# The default image (TC27x) writes only into the PF1 logical sector 0xA021C000,
# which the application does not use.  The device is detected from the JTAG ID
# by m25_4b_flash.js; an image outside the device's PFLASH is refused.
#
# Preconditions:
#   - tas_server (24817) + TCF agent (1534) running, target attached
#   - the target must be in the normal debug state (no reset+halt latch):
#     if DEVICE_CONNECT fails, power-cycle the board and retry
param(
    [string]$Elf = (Join-Path $PSScriptRoot '..\flasher\m25_flashtest_tc27x.elf'),
    [string]$Sector = '0xA021C000',
    [string]$Driver = (Join-Path $PSScriptRoot 'm25_4b_flash.js')
)
$ErrorActionPreference = 'Stop'
$work = Join-Path ([IO.Path]::GetTempPath()) "m25_flash_selftest_$PID"
New-Item -ItemType Directory -Force $work | Out-Null
$img = Join-Path $work (Split-Path $Elf -Leaf)
Copy-Item $Elf $img                      # sector backups are written next to the ELF
$env:M25_STATE = Join-Path $work 'state.json'
$orig = Join-Path $work 'sector_orig.json'
try {
    function Step([string]$name, [string[]]$argv) {
        Write-Host "=== $name ===" -ForegroundColor Cyan
        $out = & node (@($Driver) + $argv) 2>&1 | Out-String
        $code = $LASTEXITCODE
        $out.Trim().Split("`n") | Where-Object { $_.Trim() -ne '' } | Select-Object -Last 5 | ForEach-Object { "   $_" }
        if ($code -ne 0) { throw "$name failed (exit $code)" }
    }

    Step '0. backup (original sector)'          @('backup', $Sector, $orig)
    Step '1. save (state snapshot)'             @('save')
    Step '2. load (flash loader)'               @('load')
    Step '3. flash (ELF -> PFLASH, RMW+verify)' @('flash', $img)
    Step '4. restore (sector backups)'          @('flashrestore', $img)
    Step '5. verify (sector = original)'        @('verifysector', $orig)
    Step '6. stop (restore state, resume)'      @('stop')
    Write-Host "SELF-TEST PASSED (work files: $work)" -ForegroundColor Green
}
catch {
    Write-Host "SELF-TEST FAILED: $_ - the loader may still run; backups in $work" -ForegroundColor Red
    throw
}
