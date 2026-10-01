function Get-PrSniperPayloadEntry([string[]] $Listing) {
    $entries = @($Listing | Where-Object { $_ -match '^Path = (?:.*[\\/])?new-app\.exe$' } |
        ForEach-Object { $_.Substring(7) })
    if ($entries.Count -ne 1) { throw 'Expected exactly one new-app.exe in the NSIS archive.' }
    return $entries[0]
}

function Get-PrSniperInstallerPayload {
    param(
        [Parameter(Mandatory)][string] $Installer,
        [Parameter(Mandatory)][string] $Version,
        [Parameter(Mandatory)][string] $Destination,
        [string] $SevenZip
    )
    if (-not $SevenZip) {
        $command = Get-Command 7z.exe -ErrorAction SilentlyContinue
        if ($command) { $SevenZip = $command.Source }
        elseif ($env:ChocolateyInstall) { $SevenZip = Join-Path $env:ChocolateyInstall 'tools\7z.exe' }
    }
    if (-not $SevenZip -or -not (Test-Path -LiteralPath $SevenZip -PathType Leaf)) {
        throw 'An existing 7-Zip extractor is required to verify the actual NSIS payload. No tool was installed.'
    }
    $Installer = (Resolve-Path -LiteralPath $Installer -ErrorAction Stop).Path
    $listing = & $SevenZip l -slt $Installer
    if ($LASTEXITCODE -ne 0) { throw 'Cannot list the NSIS payload without executing it.' }
    $entry = Get-PrSniperPayloadEntry $listing
    New-Item -ItemType Directory -Path $Destination -ErrorAction Stop | Out-Null
    $Destination = (Resolve-Path -LiteralPath $Destination).Path
    & $SevenZip e $Installer "-o$Destination" -y -bso0 -bsp0 -- $entry
    if ($LASTEXITCODE -ne 0) { throw 'NSIS application payload extraction failed.' }
    $path = Join-Path $Destination 'new-app.exe'
    $info = [Diagnostics.FileVersionInfo]::GetVersionInfo($path)
    if ($info.ProductName -cne 'PR Sniper' -or $info.ProductVersion -cne $Version) {
        throw 'Extracted NSIS application identity/version mismatch.'
    }
    $bytes = [IO.File]::ReadAllBytes($path)
    if ($bytes.Length -lt 64 -or $bytes[0] -ne 0x4d -or $bytes[1] -ne 0x5a) {
        throw 'Extracted application is not a PE executable.'
    }
    $pe = [BitConverter]::ToUInt32($bytes, 0x3c)
    if ($pe -gt $bytes.Length - 94 -or [BitConverter]::ToUInt32($bytes, $pe) -ne 0x4550 -or
        [BitConverter]::ToUInt16($bytes, $pe + 4) -ne 0x8664 -or
        [BitConverter]::ToUInt16($bytes, $pe + 24) -ne 0x20b -or
        [BitConverter]::ToUInt16($bytes, $pe + 92) -ne 2) {
        throw 'Extracted NSIS application must be an x64 GUI executable.'
    }
    if ((Get-AuthenticodeSignature -LiteralPath $path).Status -ne 'NotSigned') {
        throw 'Expected an unsigned CI application payload, not a public signed release.'
    }
    [pscustomobject]@{
        path = $path
        version = $info.ProductVersion
        sha256 = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
    }
}
