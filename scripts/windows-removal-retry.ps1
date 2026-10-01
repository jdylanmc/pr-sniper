$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted' -or
    $env:PR_SNIPER_PACKAGING_ACCEPTANCE -ne '1') { throw 'Removal retry fixture requires owned hosted acceptance.' }
. (Join-Path $PSScriptRoot 'windows-removal-diagnostics.ps1')
$directory = Join-Path $env:LOCALAPPDATA 'PR Sniper'
$uninstaller = Join-Path $directory 'uninstall.exe'
$tools = Join-Path $env:ChocolateyInstall 'lib\pr-sniper-localtest\tools'
$receipt = Get-Content (Join-Path $tools 'installation.json') -Raw | ConvertFrom-Json
. (Join-Path $tools 'removal-state.ps1')
$receiptHash = (Get-FileHash (Join-Path $tools 'installation.json')).Hash
if ((Get-PrSniperRemovalState $tools $receipt $receiptHash).completed) { throw 'Installed package was already marked removed.' }
$attributes = [IO.File]::GetAttributes($uninstaller)
$failure = $null
try {
    [IO.File]::SetAttributes($uninstaller, $attributes -bor [IO.FileAttributes]::ReadOnly)
    & choco uninstall pr-sniper-localtest --yes --limit-output --no-progress --execution-timeout=180
    $failedExit = $LASTEXITCODE
    if ($failedExit -eq 0) { throw 'Read-only post-native deletion incorrectly succeeded.' }
    $appExists = Test-Path (Join-Path $directory 'pr-sniper.exe')
    $observedHash = (Get-FileHash $uninstaller).Hash
    $completed = (Get-PrSniperRemovalState $tools $receipt $receiptHash).completed
    try {
        $observation = Get-PrSniperRemovalRetryObservation $tools $directory $env:ChocolateyInstall `
            $receipt.version $receipt.uninstaller_sha256 $receiptHash $completed $failedExit
        $json = $observation | ConvertTo-Json -Depth 6
        $json | Set-Content (Join-Path $env:PR_SNIPER_INSTALLER_DIAGNOSTICS 'removal-after-chocolatey-failure.json') -Encoding utf8
        Write-Host $json
    } catch { Write-Warning 'Post-native diagnostic capture failed; original evidence assertion remains mandatory.' }
    if ($appExists -or $observedHash -ine $receipt.uninstaller_sha256 -or -not $completed) {
        throw "Post-native failure did not preserve the exact completion evidence: app_exists=$appExists; expected_uninstaller_sha256=$($receipt.uninstaller_sha256); observed_uninstaller_sha256=$observedHash; native_completed=$completed."
    }
} catch { $failure = $_ }
finally {
    try { [IO.File]::SetAttributes($uninstaller, $attributes) }
    catch {
        if ($failure) { Write-Warning 'Additional fixture attribute-restore failure; preserving original error.' }
        else { $failure = $_ }
    }
}
if ($failure) { throw $failure }

# A different thread owns the actual SID mutex, so the package's resume attempt
# deterministically fails at acquisition rather than recursively taking our lock.
Add-Type -TypeDefinition @'
using System;
using System.Threading;
public sealed class PrSniperCleanupLock : IDisposable {
    readonly ManualResetEventSlim ready = new ManualResetEventSlim();
    readonly ManualResetEventSlim release = new ManualResetEventSlim();
    readonly Thread worker;
    Exception failure;
    public PrSniperCleanupLock(string name) {
        worker = new Thread(() => {
            bool owned = false;
            using (var mutex = new Mutex(false, name)) {
                try {
                    try { owned = mutex.WaitOne(10000); }
                    catch (AbandonedMutexException) { owned = true; }
                    if (!owned) throw new InvalidOperationException("Fixture lock unavailable.");
                    ready.Set();
                    release.Wait(30000);
                } catch (Exception e) { failure = e; ready.Set(); }
                finally { if (owned) mutex.ReleaseMutex(); }
            }
        });
        worker.IsBackground = true;
        worker.Start();
        if (!ready.Wait(15000)) { release.Set(); throw new TimeoutException("Fixture lock timed out."); }
        if (failure != null) throw new InvalidOperationException("Fixture lock failed.", failure);
    }
    public void Dispose() {
        release.Set();
        if (!worker.Join(15000)) throw new TimeoutException("Fixture lock did not release.");
        ready.Dispose();
        release.Dispose();
    }
}
'@
$completionPath = Join-Path $tools 'native-removal.json'
$completionHash = (Get-FileHash $completionPath).Hash
$sid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
function Assert-BusyCleanupRefusal([bool] $UninstallerPresent) {
    $hold = [PrSniperCleanupLock]::new("Global\com.jdylanmc.pr-sniper.installer.$sid")
    $failure = $null
    try {
        $rejected = $false
        try { & (Join-Path $tools 'chocolateyuninstall.ps1') }
        catch {
            if ($_.Exception.Message -notmatch 'Another installer owns post-uninstall cleanup') { throw }
            $rejected = $true
        }
        if (-not $rejected -or (Get-FileHash $completionPath).Hash -cne $completionHash -or
            (Test-Path -LiteralPath $uninstaller) -ne $UninstallerPresent -or
            ($UninstallerPresent -and (Get-FileHash $uninstaller).Hash -ine $receipt.uninstaller_sha256)) {
            throw 'Busy-mutex resume did not fail with its exact completion evidence retained.'
        }
    } catch { $failure = $_ }
    finally {
        try { $hold.Dispose() }
        catch {
            if ($failure) { Write-Warning 'Additional lock-fixture cleanup failure; preserving original error.' }
            else { $failure = $_ }
        }
    }
    if ($failure) { throw $failure }
}
Assert-BusyCleanupRefusal $true
# Complete the package script without Chocolatey's outer cleanup, reproducing
# retained package/receipts after successful native removal and executable deletion.
& (Join-Path $tools 'chocolateyuninstall.ps1')
if ((Test-Path -LiteralPath $uninstaller) -or
    -not (Test-Path (Join-Path $tools 'installation.json')) -or
    (Get-FileHash $completionPath).Hash -cne $completionHash) {
    throw 'Absent-uninstaller fixture did not retain the exact package completion evidence.'
}
Assert-BusyCleanupRefusal $false
& choco uninstall pr-sniper-localtest --yes --limit-output --no-progress --execution-timeout=180
if ($LASTEXITCODE -ne 0 -or (Test-Path $uninstaller)) { throw 'Completed-removal retry did not finish.' }
if (Test-Path (Split-Path -Parent $tools)) { throw 'Outer package cleanup left tracked completion state or package files.' }
$listed = @(& choco list pr-sniper-localtest --exact --limit-output)
if ($LASTEXITCODE -ne 0 -or @($listed | Where-Object { $_.Trim() }).Count) { throw 'Final package list is not empty or could not be verified.' }
'Hosted deletion failure, present/absent mutex rejection and retained-package retry passed.'
