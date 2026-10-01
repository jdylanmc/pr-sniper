$ErrorActionPreference = 'Stop'
$tools = Split-Path -Parent $MyInvocation.MyCommand.Definition
. (Join-Path $tools 'windows-signature.ps1')
. (Join-Path $tools 'removal-state.ps1')
$receiptPath = Join-Path $tools 'installation.json'
$receiptHash = (Get-FileHash $receiptPath -Algorithm SHA256).Hash
$receipt = Get-Content $receiptPath -Raw | ConvertFrom-Json
$directory = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'PR Sniper'
$uninstaller = Join-Path $directory 'uninstall.exe'
if ($receipt.directory -cne $directory) {
    throw 'Installation receipt does not name this exact application directory.'
}
$removalState = Get-PrSniperRemovalState $tools $receipt $receiptHash
if (-not $removalState.completed) {
    if ((Get-FileHash $uninstaller -Algorithm SHA256).Hash -ine $receipt.uninstaller_sha256) {
        throw 'The exact package-owned uninstaller is unavailable or has changed.'
    }
    $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey('CurrentUser', 'Registry64')
    $key = $base.OpenSubKey('Software\Microsoft\Windows\CurrentVersion\Uninstall\PR Sniper')
    try {
        if (-not $key -or $key.GetValue('DisplayVersion') -cne $receipt.version -or
            $key.GetValue('PRSniperInstaller') -cne "com.jdylanmc.pr-sniper|$directory\pr-sniper.exe" -or
            $key.GetValue('InstallLocation') -cne $directory -or
            $key.GetValue('UninstallString') -cne "`"$uninstaller`"") {
            throw 'Installer registration no longer belongs to this exact package installation.'
        }
    } finally {
        if ($key) { $key.Dispose() }
        $base.Dispose()
    }
    # _?= must be last/unquoted. Observe the real native exit before recording
    # completion; a pending/failed native operation never gains a resume receipt.
    Start-ChocolateyProcessAsAdmin -ExeToRun $uninstaller -Statements "/S _?=$directory" `
        -Elevated:$false -ValidExitCodes @(0)
    Complete-PrSniperNativeRemoval $tools $receipt $receiptHash
}
$sid = [Security.Principal.WindowsIdentity]::GetCurrent().User.Value
$lifecycle = [Threading.Mutex]::new($false, "Global\com.jdylanmc.pr-sniper.installer.$sid")
$locked = $false
try {
    try { $locked = $lifecycle.WaitOne(0) }
    catch [Threading.AbandonedMutexException] { $locked = $true }
    if (-not $locked) { throw 'Another installer owns post-uninstall cleanup; completed-removal receipt retained for retry.' }
    if ((Get-FileHash $receiptPath -Algorithm SHA256).Hash -ine $receiptHash) {
        throw 'Installation receipt changed before cleanup.'
    }
    $removalState = Get-PrSniperRemovalState $tools $receipt $receiptHash
    if (-not $removalState.completed) { throw 'Native removal is not completed; cleanup is refused.' }
    $completion = $removalState.receipt
    Assert-PrSniperRemovalReceipt $completion $receipt $receiptHash
    $base = [Microsoft.Win32.RegistryKey]::OpenBaseKey('CurrentUser', 'Registry64')
    $key = $base.OpenSubKey('Software\Microsoft\Windows\CurrentVersion\Uninstall\PR Sniper')
    try {
        $keyExists = $null -ne $key
        $values = if ($key) { @($key.GetValueNames()) } else { @() }
        $subKeys = if ($key) { $key.SubKeyCount } else { 0 }
    } finally {
        if ($key) { $key.Dispose() }
        $base.Dispose()
    }
    $shortcut = Join-Path ([Environment]::GetFolderPath('Programs')) 'PR Sniper.lnk'
    $shortcutExists = Test-Path -LiteralPath $shortcut
    $shortcutTarget = if ($shortcutExists) { (New-Object -ComObject WScript.Shell).CreateShortcut($shortcut).TargetPath } else { '' }
    Assert-PrSniperRemovalState -AppExists (Test-Path (Join-Path $directory 'pr-sniper.exe')) `
        -TransactionExists (Test-Path (Join-Path $directory '.pr-sniper-transaction')) `
        -RegistryKeyExists $keyExists -RegistrySubKeyCount $subKeys -RegistryValueNames $values `
        -ShortcutExists $shortcutExists -ShortcutTarget $shortcutTarget -ExpectedApp (Join-Path $directory 'pr-sniper.exe')
    $fileState = Get-PrSniperUninstallerFileState $uninstaller
    if (Test-PrSniperUninstallerCleanupRequired $completion $receipt $receiptHash $fileState.exists $fileState.sha256) {
        Remove-Item -LiteralPath $uninstaller
    }
    # Empty directory removal is optional. Do not introduce a second fallible
    # cleanup after deleting the last executable; Chocolatey removes its receipts.
} finally {
    if ($locked) { $lifecycle.ReleaseMutex() }
    $lifecycle.Dispose()
}
