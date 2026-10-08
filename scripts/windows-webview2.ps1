function Get-PrSniperWebView2Version {
    $paths = @(
        'Registry::HKEY_LOCAL_MACHINE\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}',
        'Registry::HKEY_CURRENT_USER\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
    )
    foreach ($path in $paths) {
        try {
            $registration = Get-ItemProperty -LiteralPath $path -ErrorAction Stop
        } catch [System.Management.Automation.ItemNotFoundException] {
            continue
        }
        $version = $null
        if ([version]::TryParse($registration.pv, [ref]$version) -and $version -gt [version]'0.0.0.0') {
            return $version
        }
    }
}

function Wait-PrSniperWebView2Runtime {
    param([ValidateRange(0, 120)][int] $TimeoutSeconds = 60)
    Write-Host 'Checking WebView2 Evergreen registration before installing PR Sniper.'
    $clock = [Diagnostics.Stopwatch]::StartNew()
    while ($true) {
        $version = Get-PrSniperWebView2Version
        if ($version) {
            Write-Host "WebView2 Evergreen is registered: $version."
            return $version
        }
        if ($clock.Elapsed.TotalSeconds -ge $TimeoutSeconds) {
            throw 'WebView2 Evergreen did not register before the bounded timeout. Verify the webview2-runtime dependency installation, then retry PR Sniper. No application installer was started.'
        }
        Start-Sleep -Milliseconds 250
    }
}
