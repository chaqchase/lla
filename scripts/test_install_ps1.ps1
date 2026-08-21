[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$Binary
)

$ErrorActionPreference = 'Stop'
$binaryPath = (Resolve-Path $Binary).Path

. (Join-Path $PSScriptRoot '..\install.ps1')

if ((Resolve-LlaArchitecture 'AMD64' '') -ne 'amd64') { throw 'AMD64 mapping failed' }
if ((Resolve-LlaArchitecture 'AMD64' 'ARM64') -ne 'arm64') { throw 'ARM64 native mapping failed' }
$asset = 'lla-windows-amd64.exe'
$hash = 'a' * 64
if ((Get-LlaChecksum "$hash  $asset`n" $asset) -ne $hash) { throw 'Checksum parsing failed' }
try { Get-LlaChecksum "$hash  $asset.old`n" $asset | Out-Null; throw 'Inexact checksum entry was accepted' } catch {
    if ($_.Exception.Message -eq 'Inexact checksum entry was accepted') { throw }
}
try { Resolve-LlaArchitecture 'x86' '' | Out-Null; throw 'x86 was accepted' } catch {
    if ($_.Exception.Message -eq 'x86 was accepted') { throw }
}

$expectedAsset = "lla-windows-$(Resolve-LlaArchitecture).exe"
$expectedHash = (Get-FileHash -Algorithm SHA256 $binaryPath).Hash.ToLowerInvariant()
$script:webRequests = @()
function Invoke-WebRequest {
    param(
        [Parameter(Mandatory = $true)][string]$Uri,
        [Parameter(Mandatory = $true)][string]$OutFile,
        [switch]$UseBasicParsing
    )

    if (-not $UseBasicParsing) {
        throw "Invoke-WebRequest did not request basic parsing for '$Uri'."
    }
    $script:webRequests += $Uri
    if ($Uri.EndsWith('.exe')) {
        Copy-Item $binaryPath $OutFile
    } else {
        # Match Windows PowerShell 5.1 behavior for GitHub's binary checksum response
        # while ensuring the installer decodes the downloaded file explicitly.
        [IO.File]::WriteAllBytes(
            $OutFile,
            [Text.Encoding]::UTF8.GetBytes("$expectedHash  $expectedAsset`n")
        )
    }
}

$originalUserPath = [Environment]::GetEnvironmentVariable('Path', 'User')
$originalProcessPath = $env:Path
$temporaryRoot = if ([string]::IsNullOrWhiteSpace($env:RUNNER_TEMP)) {
    [IO.Path]::GetTempPath()
} else {
    $env:RUNNER_TEMP
}
$customInstall = Join-Path $temporaryRoot 'lla-custom-install'
try {
    $script:Version = 'v0.0.0-test'
    $script:InstallDir = $customInstall
    $script:NoPathUpdate = $true
    Invoke-LlaInstall
    if ($script:webRequests.Count -ne 2) { throw 'Installer did not download both required assets' }
    if (-not (Test-Path (Join-Path $customInstall 'lla.exe'))) { throw 'Custom install directory was ignored' }
    if ([Environment]::GetEnvironmentVariable('Path', 'User') -ne $originalUserPath) { throw '-NoPathUpdate changed the user PATH' }

    Add-LlaToUserPath $customInstall
    $userEntries = @([Environment]::GetEnvironmentVariable('Path', 'User') -split ';')
    if (-not ($userEntries -contains $customInstall)) { throw 'User PATH was not updated' }
    Add-LlaToUserPath $customInstall
    $matches = @([Environment]::GetEnvironmentVariable('Path', 'User') -split ';' | Where-Object { $_ -eq $customInstall })
    if ($matches.Count -ne 1) { throw 'User PATH update was not idempotent' }
} finally {
    [Environment]::SetEnvironmentVariable('Path', $originalUserPath, 'User')
    $env:Path = $originalProcessPath
    Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $customInstall
}
