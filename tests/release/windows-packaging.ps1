$ErrorActionPreference = 'Stop'
$repository = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
. (Join-Path $repository 'scripts\windows-signature.ps1')
$fixture = Join-Path $repository ('src-tauri\target\packaging-tests-' + [guid]::NewGuid())
New-Item -ItemType Directory $fixture | Out-Null
$originalTemp = $env:TEMP
$originalTmp = $env:TMP
$originalActions = $env:GITHUB_ACTIONS
$env:TEMP = $fixture
$env:TMP = $fixture
$env:GITHUB_ACTIONS = ''
$script:count = 0
function Check([bool] $Condition, [string] $Message) {
    if (-not $Condition) { throw $Message }
    $script:count++
}
function Reject([scriptblock] $Operation, [string] $Pattern) {
    $errorSeen = $null
    try { & $Operation | Out-Null } catch { $errorSeen = $_.Exception.Message }
    Check ($null -ne $errorSeen -and $errorSeen -match $Pattern) "Expected rejection '$Pattern', got '$errorSeen'."
}
try {
    $files = @(Get-ChildItem (Join-Path $repository 'scripts\windows-*.ps1')) +
        @(Get-ChildItem (Join-Path $repository 'packaging\chocolatey\*.ps1'))
    foreach ($file in $files) {
        $parseErrors = $null
        $tokens = $null
        [Management.Automation.Language.Parser]::ParseFile($file.FullName, [ref]$tokens, [ref]$parseErrors) | Out-Null
        Check ($parseErrors.Count -eq 0) "PowerShell parse errors: $($file.Name): $parseErrors"
    }
    Reject { & (Join-Path $repository 'scripts\windows-installer-acceptance.ps1') -Candidate $fixture -Upgrade $fixture } 'forbidden outside'
    Reject { & (Join-Path $repository 'scripts\windows-upgrade-fixture.ps1') } 'only on a hosted'
    $executable = Join-Path $fixture 'never-execute.exe'
    Add-Type -OutputAssembly $executable -OutputType WindowsApplication -TypeDefinition @'
using System.Reflection;
[assembly: AssemblyProduct("PR Sniper")]
[assembly: AssemblyInformationalVersion("0.1.1")]
[assembly: AssemblyFileVersion("0.1.1.0")]
public class PackagingFixture { public static void Main() {} }
'@
    $hash = (Get-FileHash $executable).Hash
    $arguments = @{
        Mode = 'LocalTest'; Installer = $executable; Version = '0.1.1'
        Sha256 = $hash; Commit = ('a' * 40); Destination = (Join-Path $fixture 'package')
    }
    $generator = Join-Path $repository 'scripts\windows-chocolatey.ps1'
    & $generator @arguments | Out-Null
    $nuspec = [xml](Get-Content (Join-Path $arguments.Destination 'pr-sniper-localtest.nuspec') -Raw)
    Check ($nuspec.package.metadata.id -ceq 'pr-sniper-localtest') 'Test package must not use the public ID.'
    Check ($nuspec.package.metadata.version -ceq '0.1.1-localtest') 'Test package must be a prerelease.'
    Check ($nuspec.package.metadata.authors -ceq 'Dylan McCurry') 'Preserve actual authorship.'
    Check (-not $nuspec.package.metadata.licenseUrl) 'Do not invent a project license.'
    Check ((Get-FileHash (Join-Path $arguments.Destination 'tools\installer.exe')).Hash -ceq $hash) 'Pin exact local bytes.'
    Reject { & (Join-Path $arguments.Destination 'tools\chocolateyinstall.ps1') } 'restricted to the disposable'
    $arguments.Destination = Join-Path $fixture 'duplicate'
    & $generator @arguments | Out-Null
    foreach ($file in Get-ChildItem (Join-Path $fixture 'package') -File -Recurse) {
        $relative = $file.FullName.Substring((Join-Path $fixture 'package').Length + 1)
        Check ((Get-FileHash $file.FullName).Hash -ceq (Get-FileHash (Join-Path $arguments.Destination $relative)).Hash) "Nondeterministic source: $relative"
    }
    $arguments.Destination = Join-Path $fixture 'rejected'
    $arguments.Sha256 = '0' * 64
    Reject { & $generator @arguments } 'checksum mismatch'
    Check (-not (Test-Path $arguments.Destination)) 'Rejected generation must not create package output.'
    $arguments.Sha256 = $hash
    $arguments.Version = '0.1.2'
    Reject { & $generator @arguments } 'identity/version mismatch'
    $arguments.Version = '0.1.1'
    $arguments.Mode = 'Public'
    Reject { & $generator @arguments } 'Public packaging requires'
    $arguments.Url = 'https://github.com/jdylanmc/pr-sniper/releases/download/v0.1.1/pr-sniper-0.1.1-x64-setup.exe'
    $arguments.ExpectedThumbprint = 'A' * 40
    $arguments.ExpectedSubject = 'CN=Test'
    $arguments.LicenseUrl = 'https://example.invalid/fixture-only'
    Reject { & $generator @arguments } 'Windows-trusted embedded Authenticode'

    . (Join-Path $repository 'scripts\windows-signature.ps1')
    $script:signature = [pscustomobject]@{
        Status = 'Valid'; SignatureType = 'Authenticode'
        TimeStamperCertificate = [pscustomobject]@{ Subject = 'CN=Timestamp' }
        SignerCertificate = [pscustomobject]@{
            Thumbprint = ('A' * 40); Subject = 'CN=Publisher'; Issuer = 'CN=Issuer'
            EnhancedKeyUsageList = @([pscustomobject]@{ ObjectId = '1.3.6.1.5.5.7.3.3' })
        }
    }
    function Get-AuthenticodeSignature { param($LiteralPath, $ErrorAction) return $script:signature }
    function Get-Command { param($Name, $ErrorAction) return @{ Source = 'Invoke-FixtureSignTool' } }
    $script:verifierCode = 0
    function Invoke-FixtureSignTool { $global:LASTEXITCODE = $script:verifierCode }
    foreach ($status in @('NotSigned', 'HashMismatch', 'NotTrusted', 'UnknownError')) {
        $signature.Status = $status
        Reject { Assert-TrustedWindowsSignature $executable ('A' * 40) 'CN=Publisher' } 'Windows-trusted'
    }
    $signature.Status = 'Valid'
    $timestamp = $signature.TimeStamperCertificate
    $signature.TimeStamperCertificate = $null
    Reject { Assert-TrustedWindowsSignature $executable ('A' * 40) 'CN=Publisher' } 'timestamp'
    $signature.TimeStamperCertificate = $timestamp
    Reject { Assert-TrustedWindowsSignature $executable ('B' * 40) 'CN=Publisher' } 'publisher differs'
    Reject { Assert-TrustedWindowsSignature $executable ('A' * 40) 'CN=Other' } 'publisher differs'
    $signature.SignerCertificate.Issuer = 'CN=Publisher'
    Reject { Assert-TrustedWindowsSignature $executable ('A' * 40) 'CN=Publisher' } 'publisher differs'
    $signature.SignerCertificate.Issuer = 'CN=Issuer'
    $signature.SignerCertificate.EnhancedKeyUsageList = @()
    Reject { Assert-TrustedWindowsSignature $executable ('A' * 40) 'CN=Publisher' } 'not authorized for code signing'
    $signature.SignerCertificate.EnhancedKeyUsageList = @([pscustomobject]@{ ObjectId = '1.3.6.1.5.5.7.3.3' })
    Assert-TrustedWindowsSignature $executable ('A' * 40) 'CN=Publisher' -RequireSignTool
    $script:verifierCode = 1
    Reject { Assert-TrustedWindowsSignature $executable ('A' * 40) 'CN=Publisher' -RequireSignTool } 'SignTool rejected'
    "Windows packaging: $count assertions passed. No installer or app executed; no registry/credential writes."
} finally {
    $env:TEMP = $originalTemp
    $env:TMP = $originalTmp
    $env:GITHUB_ACTIONS = $originalActions
    Remove-Item -LiteralPath $fixture -Recurse -Force
}
