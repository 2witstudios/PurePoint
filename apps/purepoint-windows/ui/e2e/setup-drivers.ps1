# Fetches the WebDriver pieces the end-to-end tests need (WIN-5.7) into
# apps/purepoint-windows/.e2e-tools (gitignored), reusing what is already there:
#
# - msedgedriver matching the installed WebView2 runtime, from Microsoft's driver CDN.
# - tauri-driver, built with `cargo install --locked` (the official Tauri WebDriver server).
#
# Writes .e2e-tools/drivers.json with both paths and versions for ui/e2e/run.mjs.
# Runs on Windows PowerShell 5.1 and PowerShell 7.

[CmdletBinding()]
param(
    [string]$ToolsDir = '',
    [string]$TauriDriverVersion = '2.0.6'
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

# Windows PowerShell 5.1 has no $PSScriptRoot in parameter defaults.
if (-not $ToolsDir) { $ToolsDir = Join-Path $PSScriptRoot '..\..\.e2e-tools' }
New-Item -ItemType Directory -Force -Path $ToolsDir | Out-Null
$ToolsDir = (Resolve-Path $ToolsDir).Path

function Get-WebView2Version {
    # The Evergreen runtime registers itself under EdgeUpdate, per machine or per user.
    $client = 'Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
    $keys = @(
        "HKLM:\SOFTWARE\WOW6432Node\$client",
        "HKLM:\SOFTWARE\$client",
        "HKCU:\SOFTWARE\$client"
    )
    foreach ($key in $keys) {
        $item = Get-ItemProperty -Path $key -Name pv -ErrorAction SilentlyContinue
        if ($item -and $item.pv -and $item.pv -ne '0.0.0.0') { return $item.pv }
    }
    # Fall back to the newest runtime folder on disk.
    $roots = @(
        "${env:ProgramFiles(x86)}\Microsoft\EdgeWebView\Application",
        "$env:ProgramFiles\Microsoft\EdgeWebView\Application"
    )
    foreach ($root in $roots) {
        if (-not (Test-Path $root)) { continue }
        $newest = Get-ChildItem -Path $root -Directory |
            Where-Object { $_.Name -match '^\d+\.\d+\.\d+\.\d+$' } |
            Sort-Object { [version]$_.Name } -Descending |
            Select-Object -First 1
        if ($newest) { return $newest.Name }
    }
    throw 'No WebView2 runtime found. Install the Evergreen WebView2 runtime.'
}

function Install-EdgeDriver([string]$Version) {
    $dir = Join-Path $ToolsDir "msedgedriver\$Version"
    $exe = Join-Path $dir 'msedgedriver.exe'
    if (Test-Path $exe) { return $exe }

    $arch = if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') { 'arm64' } else { 'win64' }
    $zip = Join-Path $ToolsDir "edgedriver_${arch}_$Version.zip"
    $urls = @(
        "https://msedgedriver.microsoft.com/$Version/edgedriver_$arch.zip",
        "https://msedgedriver.azureedge.net/$Version/edgedriver_$arch.zip"
    )
    $downloaded = $false
    foreach ($url in $urls) {
        try {
            Write-Host "Downloading msedgedriver $Version from $url"
            Invoke-WebRequest -Uri $url -OutFile $zip -UseBasicParsing
            $downloaded = $true
            break
        } catch {
            Write-Warning "  failed: $($_.Exception.Message)"
        }
    }
    if (-not $downloaded) { throw "Could not download msedgedriver $Version." }

    $staging = "$dir.partial"
    if (Test-Path $staging) { Remove-Item -Recurse -Force $staging }
    Expand-Archive -Path $zip -DestinationPath $staging -Force
    Remove-Item -Force $zip
    $found = Get-ChildItem -Path $staging -Recurse -Filter 'msedgedriver.exe' | Select-Object -First 1
    if (-not $found) { throw "msedgedriver.exe missing from the $Version archive." }
    if ($found.DirectoryName -ne $staging) {
        Get-ChildItem -Path $found.DirectoryName | Move-Item -Destination $staging
    }
    Move-Item -Path $staging -Destination $dir
    return $exe
}

function Install-TauriDriver([string]$Version) {
    $exe = Join-Path $ToolsDir 'bin\tauri-driver.exe'
    $stamp = Join-Path $ToolsDir 'bin\tauri-driver.version'
    if ((Test-Path $exe) -and (Test-Path $stamp) -and ((Get-Content $stamp -Raw).Trim() -eq $Version)) {
        return $exe
    }
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        throw 'cargo is needed to install tauri-driver (https://rustup.rs).'
    }
    Write-Host "Installing tauri-driver $Version into $ToolsDir"
    & cargo install tauri-driver --version $Version --locked --root $ToolsDir
    if ($LASTEXITCODE -ne 0) { throw "cargo install tauri-driver failed ($LASTEXITCODE)." }
    Set-Content -Path $stamp -Value $Version -Encoding ASCII
    return $exe
}

$webview2 = Get-WebView2Version
$edgeDriver = Install-EdgeDriver $webview2
$tauriDriver = Install-TauriDriver $TauriDriverVersion

$drivers = [ordered]@{
    webview2Version     = $webview2
    msedgedriver        = $edgeDriver
    tauriDriver         = $tauriDriver
    tauriDriverVersion  = $TauriDriverVersion
}
$json = $drivers | ConvertTo-Json
[IO.File]::WriteAllText((Join-Path $ToolsDir 'drivers.json'), $json)
Write-Host "WebView2 $webview2, msedgedriver $edgeDriver, tauri-driver $TauriDriverVersion"
