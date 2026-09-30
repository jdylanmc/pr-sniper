param([ValidateSet('Install','Uninstall')][string] $Phase, [string] $Installer, [string] $PreviousInstaller)
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted' -or
    $env:PR_SNIPER_PACKAGING_ACCEPTANCE -ne '1') { throw 'Native fault fixtures require the owned hosted acceptance job.' }
$directory = Join-Path $env:LOCALAPPDATA 'PR Sniper'
$shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'PR Sniper.lnk'
$keyPath = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\PR Sniper'
function Snapshot {
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
function Require-FailedAndPreserved {
    $before = Snapshot
    if ($Phase -eq 'Install') {
        $process = Start-Process $Installer -ArgumentList '/S' -PassThru
        if (-not $process.WaitForExit(60000)) {
            Stop-Process -Id $process.Id
            throw 'Owned fault installer timed out.'
        }
        if ($process.ExitCode -eq 0) { throw 'Faulted installer incorrectly succeeded.' }
    } else {
        & choco uninstall pr-sniper-localtest --yes --limit-output --no-progress --execution-timeout=60
        if ($LASTEXITCODE -eq 0) { throw 'Faulted Chocolatey uninstall incorrectly succeeded.' }
        if (-not (Test-Path "$env:ChocolateyInstall\lib\pr-sniper-localtest\tools\installation.json")) {
            throw 'Failed uninstall discarded its recovery receipt.'
        }
    }
    if ((Snapshot) -cne $before -or (Test-Path (Join-Path $directory '.pr-sniper-transaction'))) {
        throw 'Fault rollback did not preserve exact prior files/value types/version/shortcut.'
    }
}
if ($Phase -eq 'Install') {
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
$originalAcl = Get-Acl $keyPath
try {
    $denied = Get-Acl $keyPath
    $rule = [Security.AccessControl.RegistryAccessRule]::new(
        [Security.Principal.WindowsIdentity]::GetCurrent().User, 'SetValue', 'None', 'None', 'Deny')
    $denied.AddAccessRule($rule)
    Set-Acl $keyPath $denied
    Require-FailedAndPreserved
} finally { Set-Acl $keyPath $originalAcl }
foreach ($point in @("$($Phase.ToLowerInvariant())-files", "$($Phase.ToLowerInvariant())-registration")) {
    try {
        $env:PR_SNIPER_NSIS_TEST_FAIL = $point
        Require-FailedAndPreserved
    } finally { Remove-Item Env:PR_SNIPER_NSIS_TEST_FAIL -ErrorAction SilentlyContinue }
}
"Hosted $Phase failure/rollback fixtures passed."
