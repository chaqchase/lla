[CmdletBinding()]
param(
    [string]$Version,
    [string]$InstallDir = (Join-Path ([Environment]::GetFolderPath('LocalApplicationData')) 'Programs\lla'),
    [switch]$NoPathUpdate
)

$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
$LlaRepository = 'chaqchase/lla'

function Resolve-LlaArchitecture {
    param(
        [string]$Architecture = $env:PROCESSOR_ARCHITECTURE,
        [string]$NativeArchitecture = $env:PROCESSOR_ARCHITEW6432
    )

    $effective = if ([string]::IsNullOrWhiteSpace($NativeArchitecture)) {
        $Architecture
    } else {
        $NativeArchitecture
    }

    switch ($effective.ToUpperInvariant()) {
        'AMD64' { return 'amd64' }
        'ARM64' { return 'arm64' }
        default {
            throw "Unsupported Windows architecture '$effective'. Official lla binaries are available for AMD64 and ARM64."
        }
    }
}

function Normalize-LlaVersion {
    param([Parameter(Mandatory = $true)][string]$RequestedVersion)

    $trimmed = $RequestedVersion.Trim()
    if ([string]::IsNullOrWhiteSpace($trimmed)) {
        throw 'Version must not be empty.'
    }
    if ($trimmed.StartsWith('v')) {
        return $trimmed
    }
    return "v$trimmed"
}

function Get-LlaChecksum {
    param(
        [Parameter(Mandatory = $true)][string]$Manifest,
        [Parameter(Mandatory = $true)][string]$AssetName
    )

    $escaped = [Regex]::Escape($AssetName)
    $matches = [Regex]::Matches(
        $Manifest,
        "(?m)^(?<hash>[0-9a-fA-F]{64})[ \t]+\*?$escaped[ \t]*\r?$"
    )
    if ($matches.Count -ne 1) {
        throw "SHA256SUMS must contain exactly one checksum entry for '$AssetName'."
    }
    return $matches[0].Groups['hash'].Value.ToLowerInvariant()
}

function Add-LlaToUserPath {
    param([Parameter(Mandatory = $true)][string]$Directory)

    $normalized = [IO.Path]::GetFullPath($Directory).TrimEnd('\')
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    $entries = @(
        $userPath -split ';' |
            Where-Object { -not [string]::IsNullOrWhiteSpace($_) }
    )
    if (-not ($entries | Where-Object {
                [IO.Path]::GetFullPath($_).TrimEnd('\').Equals(
                    $normalized,
                    [StringComparison]::OrdinalIgnoreCase
                )
            })) {
        $updated = (@($entries) + $normalized) -join ';'
        [Environment]::SetEnvironmentVariable('Path', $updated, 'User')
    }

    $processEntries = @($env:Path -split ';')
    if (-not ($processEntries | Where-Object {
                -not [string]::IsNullOrWhiteSpace($_) -and
                [IO.Path]::GetFullPath($_).TrimEnd('\').Equals(
                    $normalized,
                    [StringComparison]::OrdinalIgnoreCase
                )
            })) {
        $env:Path = "$normalized;$env:Path"
    }
}

function Invoke-LlaInstall {
    $architecture = Resolve-LlaArchitecture
    $releaseTag = if ([string]::IsNullOrWhiteSpace($Version)) {
        Write-Host 'Fetching the latest lla release...'
        (Invoke-RestMethod "https://api.github.com/repos/$LlaRepository/releases/latest").tag_name
    } else {
        Normalize-LlaVersion $Version
    }

    $assetName = "lla-windows-$architecture.exe"
    $releaseBase = "https://github.com/$LlaRepository/releases/download/$releaseTag"
    $temporaryDirectory = Join-Path ([IO.Path]::GetTempPath()) ("lla-install-" + [Guid]::NewGuid())
    $downloadPath = Join-Path $temporaryDirectory $assetName
    $checksumPath = Join-Path $temporaryDirectory 'SHA256SUMS'

    New-Item -ItemType Directory -Force -Path $temporaryDirectory | Out-Null
    try {
        Write-Host "Downloading lla $releaseTag for Windows $architecture..."
        Invoke-WebRequest -UseBasicParsing -Uri "$releaseBase/$assetName" -OutFile $downloadPath
        Invoke-WebRequest -UseBasicParsing -Uri "$releaseBase/SHA256SUMS" -OutFile $checksumPath
        $checksumManifest = [IO.File]::ReadAllText($checksumPath, [Text.Encoding]::UTF8)
        $expected = Get-LlaChecksum -Manifest $checksumManifest -AssetName $assetName
        $actual = (Get-FileHash -Algorithm SHA256 $downloadPath).Hash.ToLowerInvariant()
        if ($actual -ne $expected) {
            throw "Checksum verification failed for '$assetName'. Expected $expected, got $actual."
        }

        New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
        $destination = Join-Path $InstallDir 'lla.exe'
        Copy-Item -Force $downloadPath $destination

        if (-not $NoPathUpdate) {
            Add-LlaToUserPath $InstallDir
        }

        Write-Host "lla $releaseTag installed to $destination" -ForegroundColor Green
        if ($NoPathUpdate) {
            Write-Host "Add '$InstallDir' to PATH to invoke lla by name."
        } else {
            Write-Host "The user PATH includes '$InstallDir'. Open a new terminal if this session does not see it."
        }
        Write-Host "Run 'lla init' to create your configuration."
    } finally {
        Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $temporaryDirectory
    }
}

if ($MyInvocation.InvocationName -ne '.') {
    Invoke-LlaInstall
}
