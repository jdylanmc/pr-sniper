$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted' -or
    $env:PR_SNIPER_PACKAGING_ACCEPTANCE -ne '1') { throw 'Removal retry fixture requires owned hosted acceptance.' }
$directory = Join-Path $env:LOCALAPPDATA 'PR Sniper'
$uninstaller = Join-Path $directory 'uninstall.exe'
$tools = Join-Path $env:ChocolateyInstall 'lib\pr-sniper-localtest\tools'
$receipt = Get-Content (Join-Path $tools 'installation.json') -Raw | ConvertFrom-Json
$attributes = [IO.File]::GetAttributes($uninstaller)
$failure = $null
try {
    [IO.File]::SetAttributes($uninstaller, $attributes -bor [IO.FileAttributes]::ReadOnly)
    & choco uninstall pr-sniper-localtest --yes --limit-output --no-progress --execution-timeout=180
    if ($LASTEXITCODE -eq 0) { throw 'Read-only post-native deletion incorrectly succeeded.' }
    if ((Test-Path (Join-Path $directory 'pr-sniper.exe')) -or
        (Get-FileHash $uninstaller).Hash -ine $receipt.uninstaller_sha256 -or
        -not (Test-Path (Join-Path $tools 'native-removal.json'))) {
        throw 'Post-native failure did not preserve the exact completion evidence.'
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
        (Get-FileHash $uninstaller).Hash -ine $receipt.uninstaller_sha256) {
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
& choco uninstall pr-sniper-localtest --yes --limit-output --no-progress --execution-timeout=180
if ($LASTEXITCODE -ne 0 -or (Test-Path $uninstaller)) { throw 'Completed-removal retry did not finish.' }
'Hosted post-native deletion failure, busy-mutex rejection and same-package retry passed.'
