param([ValidateSet('Install','Uninstall')][string] $Phase, [string] $Installer, [string] $PreviousInstaller)
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted' -or
    $env:PR_SNIPER_PACKAGING_ACCEPTANCE -ne '1') { throw 'Native fault fixtures require the owned hosted acceptance job.' }
. (Join-Path $PSScriptRoot 'windows-registry-acl-fixture.ps1')
. (Join-Path $PSScriptRoot 'windows-choco-test.ps1')
$directory = Join-Path $env:LOCALAPPDATA 'PR Sniper'
$shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'PR Sniper.lnk'
$keyPath = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\PR Sniper'
function Snapshot {
    foreach ($path in @(
        (Join-Path $directory '.pr-sniper-transaction'),
        (Join-Path ([Environment]::GetFolderPath('Programs')) 'PR Sniper.pr-sniper-stage.lnk'),
        (Join-Path ([Environment]::GetFolderPath('Programs')) 'PR Sniper.pr-sniper-backup.lnk')
    )) {
        if (Test-Path -LiteralPath $path) { throw "Owned transaction/staging residue remains: $path" }
    }
    $key = Get-Item $keyPath
    try {
        $values = @($key.GetValueNames() | Sort-Object | ForEach-Object {
            @{ name = $_; kind = $key.GetValueKind($_).ToString(); value = $key.GetValue($_) }
        })
    } finally { $key.Dispose() }
    @{
        app = (Get-FileHash (Join-Path $directory 'pr-sniper.exe')).Hash
        uninstaller = (Get-FileHash (Join-Path $directory 'uninstall.exe')).Hash
        shortcut = (Get-FileHash $shortcut).Hash
        values = $values
    } | ConvertTo-Json -Depth 5 -Compress
}
function Invoke-ExpectedFailure([switch] $Direct) {
    Write-Host "[Expected fault] $Phase lifecycle must refuse without changing the owned installation."
    if ($Phase -eq 'Install' -or $Direct) {
        $path = if ($Direct) { Join-Path $directory 'uninstall.exe' } else { $Installer }
        $arguments = if ($Direct) { "/S _?=$directory" } else { '/S' }
        $process = Start-Process $path -ArgumentList $arguments -PassThru
        if (-not $process.WaitForExit(60000)) {
            Stop-Process -Id $process.Id
            throw 'Owned fault installer timed out.'
        }
        if ($process.ExitCode -eq 0) { throw 'Faulted installer incorrectly succeeded.' }
    } else {
        Invoke-PrSniperChocoTest -Name 'uninstall-refusal' -Arguments @('uninstall','pr-sniper-localtest') `
            -ExpectFailure -TimeoutSeconds 60 | Out-Null
        if (-not (Test-Path "$env:ChocolateyInstall\lib\pr-sniper-localtest\tools\installation.json")) {
            throw 'Failed uninstall discarded its recovery receipt.'
        }
    }
}
function Require-FailedAndPreserved {
    $before = Snapshot
    Invoke-ExpectedFailure
    if ((Snapshot) -cne $before -or (Test-Path (Join-Path $directory '.pr-sniper-transaction'))) {
        throw 'Fault rollback did not preserve exact prior files/value types/version/shortcut.'
    }
    Write-Host '[PASS] Expected lifecycle refusal preserved the exact installation snapshot.'
}
$before = Snapshot
$exclusive = [IO.File]::Open($shortcut, 'Open', 'Read', 'None')
try {
    if ($Phase -eq 'Uninstall') { Invoke-ExpectedFailure -Direct }
    Invoke-ExpectedFailure
} finally { $exclusive.Dispose() }
if ((Snapshot) -cne $before -or (Test-Path (Join-Path $directory '.pr-sniper-transaction'))) {
    throw 'Unreadable shortcut changed the prior installation.'
}
if ($Phase -eq 'Install') {
    $before = Snapshot
    $foreignStage = Join-Path ([Environment]::GetFolderPath('Programs')) 'PR Sniper.pr-sniper-stage.lnk'
    $marker = [guid]::NewGuid().ToString()
    New-Item -ItemType File -Path $foreignStage -Value $marker -ErrorAction Stop | Out-Null
    try {
        Invoke-ExpectedFailure
        if ((Get-Content $foreignStage -Raw) -cne $marker) { throw 'Foreign staging was changed during refusal.' }
    } finally {
        if ((Test-Path $foreignStage) -and (Get-Content $foreignStage -Raw) -ceq $marker) {
            Remove-Item -LiteralPath $foreignStage
        }
    }
    if ((Snapshot) -cne $before) { throw 'Foreign-stage refusal changed the installed application.' }
    $sid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
    foreach ($command in @(
        @{ path = $PreviousInstaller; arguments = '' },
        @{ path = (Join-Path $directory 'uninstall.exe'); arguments = "_?=$directory" }
    )) {
        $start = @{ FilePath = $command.path; PassThru = $true }
        if ($command.arguments) { $start.ArgumentList = $command.arguments }
        $paused = Start-Process @start
        try {
            $deadline = [DateTime]::UtcNow.AddSeconds(20)
            do {
                $paused.Refresh()
                if ($paused.HasExited) { throw 'Owned interactive lifecycle exited before its page/lock.' }
                $probe = [Threading.Mutex]::new($false, "Global\com.jdylanmc.pr-sniper.installer.$sid")
                try {
                    try { $available = $probe.WaitOne(0) }
                    catch [Threading.AbandonedMutexException] { $available = $true }
                    if ($available) { $probe.ReleaseMutex() }
                } finally { $probe.Dispose() }
                if (-not $available) { break }
                Start-Sleep -Milliseconds 100
            } while ([DateTime]::UtcNow -lt $deadline)
            if ($available) { throw 'Interactive lifecycle never acquired its current-user lock.' }
            Require-FailedAndPreserved
        } finally {
            if (-not $paused.HasExited) { Stop-Process -Id $paused.Id; $paused.WaitForExit() }
        }
    }
}
$lockedFiles = if ($Phase -eq 'Install') {
    @((Join-Path $directory 'uninstall.exe'), $shortcut)
} else { @($shortcut) }
foreach ($file in $lockedFiles) {
    $handle = [IO.File]::Open($file, 'Open', 'Read', 'Read')
    try { Require-FailedAndPreserved } finally { $handle.Dispose() }
}
$denials = if ($Phase -eq 'Uninstall') { @('SetValue', 'Delete') } else { @('SetValue') }
foreach ($right in $denials) {
    $baseKey = [Microsoft.Win32.RegistryKey]::OpenBaseKey('CurrentUser', 'Registry64')
    $restoreKey = $null
    try {
        # Set-Acl reopens a writable key (including SetValue), which the fixture
        # deliberately denies. Retain only DACL read/change rights beforehand.
        $restoreKey = $baseKey.OpenSubKey(
            'Software\Microsoft\Windows\CurrentVersion\Uninstall\PR Sniper',
            [Microsoft.Win32.RegistryKeyPermissionCheck]::ReadWriteSubTree,
            [Security.AccessControl.RegistryRights]::ReadPermissions -bor [Security.AccessControl.RegistryRights]::ChangePermissions)
        if (-not $restoreKey) { throw 'Cannot retain the exact owned key for ACL restoration.' }
        Invoke-PrSniperRegistryAclDenial -RestoreKey $restoreKey `
            -Identity ([Security.Principal.WindowsIdentity]::GetCurrent().User) -Right $right `
            -Exercise { Require-FailedAndPreserved }
    } finally {
        if ($restoreKey) { $restoreKey.Dispose() }
        $baseKey.Dispose()
    }
}
$points = @("$($Phase.ToLowerInvariant())-files", "$($Phase.ToLowerInvariant())-registration")
if ($Phase -eq 'Install') { $points += 'install-revalidation' }
foreach ($point in $points) {
    try {
        $env:PR_SNIPER_NSIS_TEST_FAIL = $point
        Require-FailedAndPreserved
    } finally { Remove-Item Env:PR_SNIPER_NSIS_TEST_FAIL -ErrorAction SilentlyContinue }
}
"Hosted $Phase failure/rollback fixtures passed."
