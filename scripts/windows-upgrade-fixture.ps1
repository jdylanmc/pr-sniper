$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -or $env:RUNNER_ENVIRONMENT -ne 'github-hosted') {
    throw 'The disposable version fixture is built only on a hosted CI runner.'
}
$repository = Split-Path -Parent $PSScriptRoot
$target = Join-Path $repository 'src-tauri\target'
$base = Get-Content (Join-Path $target 'windows-installer-artifact\installer.json') -Raw | ConvertFrom-Json
$parts = $base.version.Split('.')
$version = "$($parts[0]).$($parts[1]).$([int]$parts[2] + 1)"
$destination = Join-Path $target 'windows-upgrade-fixture'
New-Item -ItemType Directory $destination -ErrorAction Stop | Out-Null
$configuration = Join-Path $destination 'test-version.json'
$webviewFixture = 'Software\PRSniperInstallerTests\' + [guid]::NewGuid()
$template = Join-Path $destination 'test-installer.nsi'
$definitions = "!define PR_SNIPER_TEST_BUILD`n!define WEBVIEW_MACHINE_KEY `"$webviewFixture\machine-missing`"`n!define WEBVIEW_USER_KEY `"$webviewFixture`"`n"
$definitions + (Get-Content (Join-Path $repository 'src-tauri\windows\installer.nsi') -Raw) |
    Set-Content $template -Encoding utf8
@{ version = $version; bundle = @{ windows = @{ nsis = @{ template = $template } } } } |
    ConvertTo-Json -Depth 5 | Set-Content $configuration -Encoding utf8
# No release manifest or tag changes: Tauri merges this disposable override.
& npm exec tauri build -- --ci --no-sign --bundles nsis --config $configuration -- --locked
if ($LASTEXITCODE -ne 0) { throw 'Test-only upgraded installer build failed.' }
$changes = git -C $repository status --porcelain
if ($LASTEXITCODE -ne 0 -or $changes) { throw 'Test version build unexpectedly changed source.' }
$app = Join-Path $target 'release\pr-sniper.exe'
if ([Diagnostics.FileVersionInfo]::GetVersionInfo($app).ProductVersion -cne $version) {
    throw 'Test-only application version override did not reach the PE resource.'
}
$installer = Join-Path $target "release\bundle\nsis\PR Sniper_${version}_x64-setup.exe"
if ((Get-AuthenticodeSignature $installer).Status -ne 'NotSigned') { throw 'Expected unsigned fixture.' }
Copy-Item $installer (Join-Path $destination 'upgrade-test-only.exe')
[ordered]@{
    distribution = 'disposable-upgrade-fixture-never-release'
    commit = $base.commit
    base_version = $base.version
    version = $version
    sha256 = (Get-FileHash $installer).Hash.ToLowerInvariant()
    application_sha256 = (Get-FileHash $app).Hash.ToLowerInvariant()
    webview_fixture = $webviewFixture
} | ConvertTo-Json | Set-Content (Join-Path $destination 'fixture.json') -Encoding utf8
