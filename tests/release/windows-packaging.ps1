$ErrorActionPreference = 'Stop'
$repository = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
. (Join-Path $repository 'scripts\windows-signature.ps1')
$fixture = Join-Path $repository ('src-tauri\target\packaging-tests-' + [guid]::NewGuid())
New-Item -ItemType Directory $fixture | Out-Null
$originalTemp = $env:TEMP
$originalTmp = $env:TMP
$originalActions = $env:GITHUB_ACTIONS
$originalLocalConsent = $env:PR_SNIPER_UNSIGNED_LOCAL_TEST_SHA256
$env:TEMP = $fixture
$env:TMP = $fixture
$env:GITHUB_ACTIONS = ''
$env:PR_SNIPER_UNSIGNED_LOCAL_TEST_SHA256 = ''
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
    . (Join-Path $repository 'scripts\windows-host-readiness.ps1')
    . (Join-Path $repository 'packaging\chocolatey\removal-state.ps1')
    . (Join-Path $repository 'scripts\windows-installer-payload.ps1')
    Check ((Get-PrSniperPayloadEntry @('Path = installer.exe', 'Path = $_41_\new-app.exe', 'Path = $_41_\new-uninstall.exe')) -ceq '$_41_\new-app.exe') `
        'Select the exact application entry, not the installer or uninstaller.'
    Reject { Get-PrSniperPayloadEntry @('Path = new-uninstall.exe') } 'exactly one'
    Reject { Get-PrSniperPayloadEntry @('Path = $_41_\new-app.exe', 'Path = other\new-app.exe') } 'exactly one'
    $profile = Join-Path $fixture 'owned-profile'
    New-Item -ItemType Directory (Join-Path $profile 'state') | Out-Null
    $hostFixture = [pscustomobject]@{ HasExited = $false; MainWindowHandle = [IntPtr]123 }
    $hostFixture | Add-Member ScriptMethod Refresh {}
    Check (-not (Test-PrSniperHostReady $hostFixture $profile)) 'A live helper window alone is not profile readiness.'
    [IO.File]::WriteAllText((Join-Path $profile 'state\notifications.json'), '{"profile_id":"beea9c15-e4ea-4af7-a4e5-687ff7290406","enabled":false}')
    [IO.File]::WriteAllText((Join-Path $profile 'state\diagnostics.jsonl'), '')
    Check (-not (Test-PrSniperHostReady $hostFixture $profile)) 'An empty newly created log is not ready yet.'
    [IO.File]::WriteAllText((Join-Path $profile 'state\diagnostics.jsonl'), "{`"event`":`"session_started`"}`n")
    Check (Test-PrSniperHostReady $hostFixture $profile) 'A nonzero internal HWND must not reject a ready owned host.'
    $hostFixture.HasExited = $true
    Reject { Test-PrSniperHostReady $hostFixture $profile } 'host exited'
    Assert-PrSniperRemovalState -AppExists $false -TransactionExists $false -RegistryValueNames @('ForeignFixture') `
        -ShortcutExists $true -ShortcutTarget 'C:\foreign.exe' -ExpectedApp 'C:\owned\pr-sniper.exe'
    foreach ($name in @('PRSniperInstaller','DisplayVersion','UninstallString','NoModify','DisplayName')) {
        Reject { Assert-PrSniperRemovalState -AppExists $false -TransactionExists $false -RegistryValueNames @($name) `
            -ShortcutExists $false -ExpectedApp 'C:\owned\pr-sniper.exe' } 'registration remains'
    }
    Reject { Assert-PrSniperRemovalState -AppExists $true -TransactionExists $false -RegistryValueNames @() `
        -ShortcutExists $false -ExpectedApp 'C:\owned\pr-sniper.exe' } 'Application removal'
    Reject { Assert-PrSniperRemovalState -AppExists $false -TransactionExists $true -RegistryValueNames @() `
        -ShortcutExists $false -ExpectedApp 'C:\owned\pr-sniper.exe' } 'transaction remains'
    Reject { Assert-PrSniperRemovalState -AppExists $false -TransactionExists $false -RegistryValueNames @() `
        -ShortcutExists $true -ShortcutTarget 'C:\owned\pr-sniper.exe' -ExpectedApp 'C:\owned\pr-sniper.exe' } 'Owned shortcut remains'
    Reject { Assert-PrSniperRemovalState -RegistryKeyExists $true -RegistryValueNames $null `
        -ExpectedApp 'C:\owned\pr-sniper.exe' } 'Empty owned installer key remains'
    Assert-PrSniperRemovalState -RegistryKeyExists $true -RegistrySubKeyCount 1 -RegistryValueNames @() `
        -ExpectedApp 'C:\owned\pr-sniper.exe'
    Assert-PrSniperRemovalState -RegistryKeyExists $true -RegistryValueNames @('') `
        -ExpectedApp 'C:\owned\pr-sniper.exe'
    Check $true 'The unnamed default registry value is foreign nonempty content.'
    $removalReceipt = [pscustomobject]@{ directory = 'C:\owned'; version = '0.1.1'; uninstaller_sha256 = ('a' * 64) }
    $completion = [pscustomobject]@{
        schema = 1; phase = 'native-removal-complete'; installation_receipt_sha256 = ('b' * 64)
        directory = 'C:\owned'; version = '0.1.1'; uninstaller_sha256 = ('a' * 64)
    }
    Assert-PrSniperRemovalReceipt $completion $removalReceipt ('b' * 64)
    Check (-not (Test-PrSniperUninstallerCleanupRequired $completion $removalReceipt ('b' * 64) $false '')) `
        'Valid native completion permits already-absent uninstaller cleanup without deletion.'
    Check (Test-PrSniperUninstallerCleanupRequired $completion $removalReceipt ('b' * 64) $true ('a' * 64)) `
        'An exact remaining uninstaller still requires owned deletion.'
    Reject { Test-PrSniperUninstallerCleanupRequired $null $removalReceipt ('b' * 64) $false '' } 'not evidenced'
    Reject { Test-PrSniperUninstallerCleanupRequired $completion $removalReceipt ('c' * 64) $false '' } 'not evidenced'
    Reject { Test-PrSniperUninstallerCleanupRequired $completion $removalReceipt ('b' * 64) $true ('c' * 64) } 'Uninstaller changed'
    $completion.uninstaller_sha256 = ''
    $removalReceipt.uninstaller_sha256 = ''
    Reject { Test-PrSniperUninstallerCleanupRequired $completion $removalReceipt ('b' * 64) $false '' } 'not evidenced'
    $completion.uninstaller_sha256 = 'a' * 64
    $removalReceipt.uninstaller_sha256 = 'a' * 64
    $missingUninstaller = Get-PrSniperUninstallerFileState (Join-Path $fixture 'absent-uninstaller.exe')
    Check (-not $missingUninstaller.exists -and $missingUninstaller.sha256 -ceq '') 'Only actual file absence produces absent evidence.'
    Reject { Get-PrSniperUninstallerFileState $fixture } 'not a regular file'
    Reject { Assert-PrSniperRemovalReceipt $null $removalReceipt ('b' * 64) } 'not evidenced'
    foreach ($field in @('schema','phase','directory','version','uninstaller_sha256','installation_receipt_sha256')) {
        $old = $completion.$field
        $completion.$field = 'wrong-or-pending'
        Reject { Assert-PrSniperRemovalReceipt $completion $removalReceipt ('b' * 64) } 'not evidenced'
        $completion.$field = $old
    }
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
    Reject { & (Join-Path $repository 'scripts\windows-installer-faults.ps1') -Phase Install -Installer 'not-executed.exe' } 'require the owned hosted'
    Reject { & (Join-Path $repository 'scripts\windows-removal-retry.ps1') } 'requires owned hosted'
    $executable = Join-Path $fixture 'never-execute.exe'
    Add-Type -OutputAssembly $executable -OutputType WindowsApplication -TypeDefinition @'
using System.Reflection;
[assembly: AssemblyProduct("PR Sniper")]
[assembly: AssemblyInformationalVersion("0.1.1")]
[assembly: AssemblyFileVersion("0.1.1.0")]
public class PackagingFixture { public static void Main() {} }
'@
    $hash = (Get-FileHash $executable).Hash
    $presentUninstaller = Get-PrSniperUninstallerFileState $executable
    Check ($presentUninstaller.exists -and $presentUninstaller.sha256 -ceq $hash) 'Present-file observation includes its actual SHA256.'
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
    $installScript = Join-Path $arguments.Destination 'tools\chocolateyinstall.ps1'
    Reject { & $installScript } 'explicit consent for this exact installer'
    $env:PR_SNIPER_UNSIGNED_LOCAL_TEST_SHA256 = '0' * 64
    Reject { & $installScript } 'explicit consent for this exact installer'
    function Start-ChocolateyProcessAsAdmin { throw 'Fixture boundary reached; no executable started.' }
    $env:PR_SNIPER_UNSIGNED_LOCAL_TEST_SHA256 = $hash
    Reject { & $installScript } 'Fixture boundary reached'
    $metadataPath = Join-Path $arguments.Destination 'tools\installer.json'
    $originalMetadata = [IO.File]::ReadAllBytes($metadataPath)
    $publicMetadata = Get-Content $metadataPath -Raw | ConvertFrom-Json
    $publicMetadata.mode = 'Public'
    $publicMetadata.url = 'https://github.com/jdylanmc/pr-sniper/releases/download/v0.1.1/pr-sniper-0.1.1-x64-setup.exe'
    $publicMetadata.signer_thumbprint = 'A' * 40
    $publicMetadata.signer_subject = 'CN=Fixture'
    $publicMetadata | ConvertTo-Json | Set-Content $metadataPath -Encoding utf8
    function Get-ChocolateyWebFile { }
    Reject { & $installScript } 'Windows-trusted embedded Authenticode'
    [IO.File]::WriteAllBytes($metadataPath, $originalMetadata)
    $env:PR_SNIPER_UNSIGNED_LOCAL_TEST_SHA256 = ''
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
    $env:PR_SNIPER_UNSIGNED_LOCAL_TEST_SHA256 = $originalLocalConsent
    Remove-Item -LiteralPath $fixture -Recurse -Force
}
