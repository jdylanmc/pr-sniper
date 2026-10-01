function Assert-PrSniperRemovalState {
    param(
        [bool] $AppExists,
        [bool] $TransactionExists,
        [bool] $RegistryKeyExists = $false,
        [int] $RegistrySubKeyCount = 0,
        [AllowEmptyCollection()][string[]] $RegistryValueNames,
        [bool] $ShortcutExists,
        [string] $ShortcutTarget,
        [string] $ExpectedApp
    )
    if ($AppExists) { throw 'Application removal did not complete; recovery executable/receipt retained.' }
    if ($TransactionExists) { throw 'Installer transaction remains; recovery executable/receipt retained.' }
    $ownedValues = @('PRSniperInstaller','DisplayName','DisplayVersion','DisplayIcon','Publisher',
        'InstallLocation','UninstallString','QuietUninstallString','NoModify','NoRepair')
    if (@($RegistryValueNames | Where-Object { $_ -in $ownedValues }).Count) {
        throw 'Owned installer registration remains; recovery executable/receipt retained.'
    }
    if ($RegistryKeyExists -and $RegistrySubKeyCount -eq 0 -and
        @($RegistryValueNames | Where-Object { $null -ne $_ }).Count -eq 0) {
        throw 'Empty owned installer key remains; recovery executable/receipt retained.'
    }
    if ($ShortcutExists -and (-not $ShortcutTarget -or $ShortcutTarget -ieq $ExpectedApp)) {
        throw 'Owned shortcut remains or cannot be resolved; recovery executable/receipt retained.'
    }

}

function Assert-PrSniperRemovalBinding($Completion, $Receipt, [string] $ReceiptHash) {
    if ($Receipt.uninstaller_sha256 -notmatch '^[a-fA-F0-9]{64}$' -or
        [string]::IsNullOrEmpty($Receipt.version) -or
        $Completion.installation_receipt_sha256 -ine $ReceiptHash -or
        $Completion.directory -cne $Receipt.directory -or $Completion.version -cne $Receipt.version -or
        $Completion.uninstaller_sha256 -ine $Receipt.uninstaller_sha256) {
        throw 'Completed native removal is not evidenced by this exact installation receipt.'
    }
}

function Assert-PrSniperRemovalReceipt($Completion, $Receipt, [string] $ReceiptHash) {
    if ($Completion.schema -ne 1 -or $Completion.phase -cne 'native-removal-complete') {
        throw 'Completed native removal is not evidenced by this exact installation receipt.'
    }
    Assert-PrSniperRemovalBinding $Completion $Receipt $ReceiptHash
}

function Initialize-PrSniperRemovalState([string] $Tools, $Receipt, [string] $ReceiptHash) {
    $statePath = Join-Path $Tools 'native-removal.json'
    $pendingPath = Join-Path $Tools 'native-removal.pending.json'
    if ((Test-Path -LiteralPath $statePath) -or (Test-Path -LiteralPath $pendingPath)) {
        throw 'Existing removal state requires exact reconciliation; nothing was overwritten.'
    }
    # Both immutable files exist before Chocolatey's install snapshot. Completion
    # deletes only the pending marker; the state file's tracked checksum never changes.
    foreach ($phase in @('native-removal-pending', 'native-removal-state')) {
        $path = if ($phase -eq 'native-removal-pending') { $pendingPath } else { $statePath }
        $record = [ordered]@{
            schema = 2
            phase = $phase
            installation_receipt_sha256 = $ReceiptHash
            directory = $Receipt.directory
            version = $Receipt.version
            uninstaller_sha256 = $Receipt.uninstaller_sha256
        }
        Assert-PrSniperRemovalBinding ([pscustomobject]$record) $Receipt $ReceiptHash
        $bytes = [Text.Encoding]::UTF8.GetBytes(($record | ConvertTo-Json))
        $stream = [IO.File]::Open($path, 'CreateNew', 'Write', 'None')
        try { $stream.Write($bytes, 0, $bytes.Length); $stream.Flush($true) }
        finally { $stream.Dispose() }
    }
}

function Get-PrSniperDurableRemovalPath(
    [string] $Tools, [string] $ChocolateyRoot, $Receipt, [string] $ReceiptHash
) {
    $installationId = [guid]::Empty
    if (-not [guid]::TryParseExact([string]$Receipt.installation_id, 'D', [ref]$installationId) -or
        $installationId -eq [guid]::Empty -or $ReceiptHash -notmatch '^[a-fA-F0-9]{64}$') {
        throw 'Durable removal requires a uniquely identified installation receipt; reconcile legacy state explicitly.'
    }
    if (-not [IO.Path]::IsPathRooted($ChocolateyRoot) -or -not [IO.Path]::IsPathRooted($Tools)) {
        throw 'Removal state requires the exact Chocolatey package location.'
    }
    $root = [IO.Path]::GetFullPath($ChocolateyRoot)
    $package = Split-Path (Split-Path $Tools -Parent) -Leaf
    if ($package -cnotin @('pr-sniper','pr-sniper-localtest') -or
        [IO.Path]::GetFullPath($Tools).TrimEnd('\') -ine (Join-Path $root "lib\$package\tools")) {
        throw 'Removal state requires the exact Chocolatey package location.'
    }
    $failedRoot = Join-Path $root 'lib-bad'
    $packageFailedRoot = Join-Path $failedRoot $package
    foreach ($path in @($root, $failedRoot, $packageFailedRoot)) {
        try { $attributes = [IO.File]::GetAttributes($path) }
        catch [IO.FileNotFoundException], [IO.DirectoryNotFoundException] { continue }
        if (-not ($attributes -band [IO.FileAttributes]::Directory) -or
            ($attributes -band [IO.FileAttributes]::ReparsePoint)) {
            throw 'Removal state location is not an ordinary Chocolatey directory.'
        }
    }
    # Failure replaces lib-bad/package/version, not this sibling. Chocolatey's
    # successful uninstall cleanup owns deletion of the package's lib-bad tree.
    return Join-Path $packageFailedRoot "native-removal-$($ReceiptHash.ToLowerInvariant()).json"
}

function Read-PrSniperDurableRemoval([string] $Path, $Receipt, [string] $ReceiptHash) {
    try { $attributes = [IO.File]::GetAttributes($Path) }
    catch [IO.FileNotFoundException], [IO.DirectoryNotFoundException] { return $null }
    if ($attributes -band ([IO.FileAttributes]::Directory -bor [IO.FileAttributes]::ReparsePoint)) {
        throw 'Durable completion is not an ordinary receipt file.'
    }
    $completion = [IO.File]::ReadAllText($Path) | ConvertFrom-Json
    Assert-PrSniperRemovalReceipt $completion $Receipt $ReceiptHash
    return $completion
}

function Get-PrSniperRemovalState(
    [string] $Tools, $Receipt, [string] $ReceiptHash, [string] $DurablePath
) {
    $state = [IO.File]::ReadAllText((Join-Path $Tools 'native-removal.json')) | ConvertFrom-Json
    if ($state.schema -ne 2 -or $state.phase -cne 'native-removal-state') {
        throw 'Legacy/unrecognized removal state needs explicit reconciliation; it was not adopted.'
    }
    Assert-PrSniperRemovalBinding $state $Receipt $ReceiptHash
    $pendingText = $null
    try { $pendingText = [IO.File]::ReadAllText((Join-Path $Tools 'native-removal.pending.json')) }
    catch [IO.FileNotFoundException] { }
    if ($null -ne $pendingText) {
        $pending = $pendingText | ConvertFrom-Json
        if ($pending.schema -ne 2 -or $pending.phase -cne 'native-removal-pending') {
            throw 'Invalid pending-removal marker; native completion is unknown.'
        }
        Assert-PrSniperRemovalBinding $pending $Receipt $ReceiptHash
    }
    if ($DurablePath) {
        $durable = Read-PrSniperDurableRemoval $DurablePath $Receipt $ReceiptHash
        if ($null -ne $durable) {
            return [pscustomobject]@{ completed = $true; receipt = $durable }
        }
    }
    if ($null -ne $pendingText) {
        return [pscustomobject]@{ completed = $false; receipt = $null }
    }
    # The validated installed state plus committed absence of its pending marker
    # is completion evidence; neither registry absence nor a missing state file is.
    $completion = [pscustomobject]@{
        schema = 1; phase = 'native-removal-complete'
        installation_receipt_sha256 = $state.installation_receipt_sha256
        directory = $state.directory; version = $state.version
        uninstaller_sha256 = $state.uninstaller_sha256
    }
    Assert-PrSniperRemovalReceipt $completion $Receipt $ReceiptHash
    return [pscustomobject]@{ completed = $true; receipt = $completion }
}

function Complete-PrSniperNativeRemoval(
    [string] $Tools, $Receipt, [string] $ReceiptHash, [string] $DurablePath
) {
    $state = Get-PrSniperRemovalState $Tools $Receipt $ReceiptHash
    if ($state.completed) { throw 'Native removal was already committed; it must not be repeated.' }
    if ($DurablePath) {
        $completion = Read-PrSniperDurableRemoval $DurablePath $Receipt $ReceiptHash
        if ($null -eq $completion) {
            $completion = [ordered]@{
                schema = 1; phase = 'native-removal-complete'
                installation_receipt_sha256 = $ReceiptHash
                directory = $Receipt.directory; version = $Receipt.version
                uninstaller_sha256 = $Receipt.uninstaller_sha256
            }
            Assert-PrSniperRemovalReceipt ([pscustomobject]$completion) $Receipt $ReceiptHash
            [IO.Directory]::CreateDirectory((Split-Path $DurablePath -Parent)) | Out-Null
            $bytes = [Text.Encoding]::UTF8.GetBytes(($completion | ConvertTo-Json))
            $stream = [IO.File]::Open($DurablePath, 'CreateNew', 'Write', 'None')
            try { $stream.Write($bytes, 0, $bytes.Length); $stream.Flush($true) }
            finally { $stream.Dispose() }
        }
        if ($null -eq (Read-PrSniperDurableRemoval $DurablePath $Receipt $ReceiptHash)) {
            throw 'Durable completion was not persisted; recovery requires exact reconciliation.'
        }
    }
    Remove-Item -LiteralPath (Join-Path $Tools 'native-removal.pending.json') -ErrorAction Stop
    if (-not (Get-PrSniperRemovalState $Tools $Receipt $ReceiptHash).completed) {
        throw 'Native removal completion was not committed.'
    }
}

function Test-PrSniperUninstallerCleanupRequired(
    $Completion, $Receipt, [string] $ReceiptHash, [bool] $UninstallerExists, [string] $UninstallerHash
) {
    Assert-PrSniperRemovalReceipt $Completion $Receipt $ReceiptHash
    if (-not $UninstallerExists) { return $false }
    if ($UninstallerHash -ine $Receipt.uninstaller_sha256) { throw 'Uninstaller changed before cleanup.' }
    return $true
}

function Get-PrSniperUninstallerFileState([string] $Path) {
    try { $attributes = [IO.File]::GetAttributes($Path) }
    catch [IO.FileNotFoundException], [IO.DirectoryNotFoundException] {
        return @{ exists = $false; sha256 = '' }
    }
    if ($attributes -band [IO.FileAttributes]::Directory) { throw 'Uninstaller path is not a regular file.' }
    return @{ exists = $true; sha256 = (Get-FileHash -LiteralPath $Path -Algorithm SHA256 -ErrorAction Stop).Hash }
}
