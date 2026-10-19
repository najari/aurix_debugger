# m25_flash_task.ps1 - flash an ELF with the M25 flasher before a VS Code debug
# session (launch.json "preLaunchTask").  Replaces the extension's
# "enableFlashing" (Infineon AURIX Flasher), which does not support every
# device stepping (e.g. TC27x JTAG ID 0x401DA083).
#
#   1. start a TCF agent unless one already listens on the port
#   2. flashcheck: nothing to do when PFLASH already matches the ELF
#   3. otherwise: save -> load (loader) -> flash (backup + erase + program +
#      verify per sector) -> stop (loader exit, ENDINIT and state restored)
#      -> reset + run, so the new image boots
#   4. no write needed: resume the cores the agent halted on attach
#   5. stop the agent again if this script started it - the extension then
#      starts its own agent for the debug session as usual
#
# Exit code 0 = target ready, anything else = VS Code does not start the session.
# Sector backups of every write go to -BackupDir (restore: m25_4b_flash.js
# flashrestore <elf> with M25_BACKUP_DIR=<BackupDir>).
param(
    [Parameter(Mandatory = $true)][string]$Elf,
    [Parameter(Mandatory = $true)][string]$Agent,
    [Parameter(Mandatory = $true)][string]$Device,
    [string]$BackupDir = (Join-Path (Split-Path $Elf) '.m25_flash_backup'),
    [int]$Port = 1534,
    [switch]$NoReset,
    # put the sectors written by the last flash of this ELF back from -BackupDir
    [switch]$Restore
)
$ErrorActionPreference = 'Stop'
$tool = Join-Path $PSScriptRoot 'm25_4b_flash.js'
$env:M25_BACKUP_DIR = $BackupDir
$env:M25_STATE = Join-Path ([IO.Path]::GetTempPath()) 'm25_flash_task_state.json'

function Test-Port {
    $c = New-Object Net.Sockets.TcpClient
    try { $c.Connect('127.0.0.1', $Port); return $true } catch { return $false } finally { $c.Close() }
}
function Run([string[]]$argv, [int[]]$ok = @(0)) {
    Write-Host "> m25_4b_flash.js $($argv -join ' ')" -ForegroundColor Cyan
    & node (@($tool) + $argv) | Out-Host
    $code = $LASTEXITCODE
    if ($ok -notcontains $code) { throw "m25_4b_flash.js $($argv[0]) failed (exit $code)" }
    return $code
}

if (-not (Test-Path $Elf)) { throw "ELF not found: $Elf" }
$started = $null
try {
    if (Test-Port) {
        Write-Host "using the TCF agent already listening on port $Port"
    } else {
        Write-Host "starting TCF agent: $Agent"
        $started = Start-Process -FilePath $Agent -PassThru -WindowStyle Hidden `
            -ArgumentList @('-s', "TCP:127.0.0.1:$Port", '-p', "`"$Device`"")
        $t0 = Get-Date
        while (-not (Test-Port)) {
            if ($started.HasExited) { throw "TCF agent exited (code $($started.ExitCode))" }
            if (((Get-Date) - $t0).TotalSeconds -gt 20) { throw 'TCF agent did not start listening' }
            Start-Sleep -Milliseconds 300
        }
        Start-Sleep 1                       # attach to the target
    }

    $check = if ($Restore) { 3 } else { Run @('flashcheck', $Elf) @(0, 3) }
    if ($Restore) {
        Run @('save') | Out-Null
        Run @('load') | Out-Null
        try {
            Run @('flashrestore', $Elf) | Out-Null
        } finally {
            Run @('stop') | Out-Null
        }
        if ($NoReset) { Run @('resumeall') | Out-Null } else { Run @('reset', 'run') | Out-Null }
        Write-Host "restored the sectors of $Elf from $BackupDir" -ForegroundColor Green
    } elseif ($check -eq 0) {
        Write-Host 'PFLASH already matches the ELF - nothing to flash' -ForegroundColor Green
        Run @('resumeall') | Out-Null
    } else {
        Run @('save') | Out-Null
        Run @('load') | Out-Null
        try {
            Run @('flash', $Elf) | Out-Null
        } finally {
            Run @('stop') | Out-Null       # always leave the loader and restore ENDINIT/state
        }
        if ($NoReset) {
            Run @('resumeall') | Out-Null
        } else {
            Run @('reset', 'run') | Out-Null
        }
        Write-Host "flashed $Elf (sector backups: $BackupDir)" -ForegroundColor Green
    }
}
finally {
    if ($started -and -not $started.HasExited) {
        Stop-Process -Id $started.Id -Confirm:$false
        Start-Sleep 1
    }
}
