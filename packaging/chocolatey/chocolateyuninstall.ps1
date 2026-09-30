$ErrorActionPreference = 'Stop'
$tools = Split-Path -Parent $MyInvocation.MyCommand.Definition
. (Join-Path $tools 'windows-signature.ps1')
. (Join-Path $tools 'removal-state.ps1')
$receiptPath = Join-Path $tools 'installation.json'
$completionPath = Join-Path $tools 'native-removal.json'
$receiptHash = (Get-FileHash $receiptPath -Algorithm SHA256).Hash
$receipt = Get-Content $receiptPath -Raw | ConvertFrom-Json
$directory = Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'PR Sniper'
$uninstaller = Join-Path $directory 'uninstall.exe'
if ($receipt.directory -cne $directory -or
    (Get-FileHash $uninstaller -Algorithm SHA256).Hash -ine $receipt.uninstaller_sha256) {
    throw 'The exact package-owned uninstaller is unavailable or has changed.'
}
if (Test-Path -LiteralPath $completionPath) {
    # Missing registration alone never admits cleanup or execution of a leftover.
    Assert-PrSniperRemovalReceipt (Get-Content $completionPath -Raw | ConvertFrom-Json) $receipt $receiptHash
} else {
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
    $completion = [ordered]@{
        schema = 1
        phase = 'native-removal-complete'
        installation_receipt_sha256 = $receiptHash
        directory = $receipt.directory
        version = $receipt.version
        uninstaller_sha256 = $receipt.uninstaller_sha256
    } | ConvertTo-Json
    $stream = [IO.File]::Open($completionPath, 'CreateNew', 'Write', 'None')
    try {
        $bytes = [Text.Encoding]::UTF8.GetBytes($completion)
        $stream.Write($bytes, 0, $bytes.Length)
        $stream.Flush($true)
    } finally { $stream.Dispose() }
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
    Assert-PrSniperRemovalReceipt (Get-Content $completionPath -Raw | ConvertFrom-Json) $receipt $receiptHash
    if ((Get-FileHash $uninstaller -Algorithm SHA256).Hash -ine $receipt.uninstaller_sha256) {
        throw 'Uninstaller changed before cleanup.'
    }
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
    Remove-Item -LiteralPath $uninstaller
    # Empty directory removal is optional. Do not introduce a second fallible
    # cleanup after deleting the last executable; Chocolatey removes its receipts.
} finally {
    if ($locked) { $lifecycle.ReleaseMutex() }
    $lifecycle.Dispose()
}
