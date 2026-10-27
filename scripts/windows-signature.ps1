if ($PSVersionTable.PSVersion.Major -eq 5) {
    # npm/Chocolatey can inherit PowerShell 7's module path. Load the inbox
    # Windows PowerShell helpers, not incompatible modules from that parent.
    Import-Module "$PSHOME\Modules\Microsoft.PowerShell.Utility\Microsoft.PowerShell.Utility.psd1" -ErrorAction Stop
    Import-Module "$PSHOME\Modules\Microsoft.PowerShell.Security\Microsoft.PowerShell.Security.psd1" -ErrorAction Stop
}

function Assert-UnsignedWindowsArtifact {
    param([Parameter(Mandatory)][string] $Path)
    $signature = Get-AuthenticodeSignature -LiteralPath $Path -ErrorAction Stop
    if ($signature.Status -ne 'NotSigned' -or $signature.SignerCertificate -or
        $signature.TimeStamperCertificate) {
        throw 'The explicit unsigned distribution mode requires an unsigned artifact; signed, invalid or untrusted signatures are not an unsigned fallback.'
    }
}

function Assert-TrustedWindowsSignature {
    param(
        [Parameter(Mandatory)][string] $Path,
        [Parameter(Mandatory)][ValidatePattern('^[A-Fa-f0-9]{40}$')][string] $ExpectedThumbprint,
        [Parameter(Mandatory)][string] $ExpectedSubject,
        [switch] $RequireSignTool
    )
    $signature = Get-AuthenticodeSignature -LiteralPath $Path -ErrorAction Stop
    if ($signature.Status -ne 'Valid' -or $signature.SignatureType -ne 'Authenticode' -or
        -not $signature.SignerCertificate -or -not $signature.TimeStamperCertificate) {
        throw 'Expected a Windows-trusted embedded Authenticode signature with a trusted timestamp.'
    }
    $certificate = $signature.SignerCertificate
    if ($certificate.Thumbprint -ine $ExpectedThumbprint -or
        $certificate.Subject -cne $ExpectedSubject -or $certificate.Subject -eq $certificate.Issuer) {
        throw 'Authenticode publisher differs from the independently configured trusted identity.'
    }
    $eku = @($certificate.EnhancedKeyUsageList | ForEach-Object { $_.ObjectId })
    if ('1.3.6.1.5.5.7.3.3' -notin $eku) {
        throw 'Publisher certificate is not authorized for code signing.'
    }
    # WinVerifyTrust (above) validates the timestamp and signing-time chain.
    # SignTool supplies an independent policy check; no trust roots are imported.
    if ($RequireSignTool) {
        $tool = Get-Command signtool.exe -ErrorAction Stop
        & $tool.Source verify /pa /all /tw $Path | Out-Null
        if ($LASTEXITCODE -ne 0) { throw 'SignTool rejected the signature or timestamp.' }
    }
}
