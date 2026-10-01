function Get-PrSniperRemovalDiagnosticFile([string] $Path) {
    try { $attributes = [IO.File]::GetAttributes($Path) }
    catch [IO.FileNotFoundException], [IO.DirectoryNotFoundException] {
        return [ordered]@{ exists = $false; sha256 = $null }
    }
    if ($attributes -band [IO.FileAttributes]::Directory) { throw 'Diagnostic file path is a directory.' }
    return [ordered]@{ exists = $true; sha256 = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant() }
}

function Get-PrSniperRemovalDiagnosticState([string] $Tools) {
    $files = [ordered]@{}
    foreach ($name in @('installation.json', 'native-removal.json', 'native-removal.pending.json')) {
        $files[$name] = Get-PrSniperRemovalDiagnosticFile (Join-Path $Tools $name)
    }
    return $files
}

function Get-PrSniperRemovalRetryObservation(
    [string] $Tools, [string] $Directory, [string] $ChocolateyRoot,
    [string] $Version, [string] $ExpectedUninstallerHash, [string] $ExpectedReceiptHash,
    [bool] $Completed, [int] $ExitCode, [string] $DurablePath
) {
    if ($Version -notmatch '^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$') {
        throw 'Invalid version for exact removal diagnostic paths.'
    }
    $relative = "pr-sniper-localtest\$Version-localtest\tools"
    return [ordered]@{
        phase = 'after-failed-chocolatey-uninstall'
        chocolatey_exit_code = $ExitCode
        expected = [ordered]@{
            app_exists = $false
            uninstaller_sha256 = $ExpectedUninstallerHash.ToLowerInvariant()
            installation_receipt_sha256 = $ExpectedReceiptHash.ToLowerInvariant()
            native_completed = $true
        }
        observed = [ordered]@{
            app = Get-PrSniperRemovalDiagnosticFile (Join-Path $Directory 'pr-sniper.exe')
            uninstaller = Get-PrSniperRemovalDiagnosticFile (Join-Path $Directory 'uninstall.exe')
            native_completed = $Completed
            durable_completion = if ($DurablePath) { Get-PrSniperRemovalDiagnosticFile $DurablePath } else { $null }
            active = Get-PrSniperRemovalDiagnosticState $Tools
            failed_copy = Get-PrSniperRemovalDiagnosticState (Join-Path $ChocolateyRoot "lib-bad\$relative")
            backup_copy = Get-PrSniperRemovalDiagnosticState (Join-Path $ChocolateyRoot "lib-bkp\$relative")
        }
    }
}
