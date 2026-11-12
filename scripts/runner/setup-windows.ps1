# Install or remove the newtype Windows x64 GitHub Actions runner.
# The registration token is a parameter or a hidden prompt. It is not written to disk.
[CmdletBinding()]
param(
    [string]$Token,
    [switch]$Uninstall,
    [string]$RunnerRoot = (Join-Path $env:USERPROFILE 'actions-runners\newtype-win')
)

# Same value as rust-version in src-tauri/Cargo.toml. scripts/runner-msrv.test.mjs checks it.
# This script is downloaded on its own, so it must not read the repo.
$MinimumRust = '1.77.2'

$ErrorActionPreference = 'Stop'
if (Get-Variable -Name PSNativeCommandUseErrorActionPreference -ErrorAction SilentlyContinue) {
    $PSNativeCommandUseErrorActionPreference = $false
}
$RepoUrl = 'https://github.com/kurlez/newtype'
$RunnerName = 'newtype-win'

function Test-Administrator {
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($identity)
    return $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function ConvertTo-PlainText {
    param([Security.SecureString]$Secure)
    $pointer = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($Secure)
    try {
        return [Runtime.InteropServices.Marshal]::PtrToStringBSTR($pointer)
    } finally {
        [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($pointer)
    }
}

function Read-Secret {
    param([string]$Label)
    $secure = Read-Host -AsSecureString $Label
    $plain = ConvertTo-PlainText $secure
    if ([string]::IsNullOrWhiteSpace($plain)) {
        throw "A token is required."
    }
    return $plain
}

function Test-WingetId {
    param([string]$Id)
    try {
        $output = & winget list --id $Id -e --accept-source-agreements 2>&1 | Out-String
        return $output.Contains($Id)
    } catch {
        return $false
    }
}

function Install-WingetId {
    param(
        [string]$Id,
        [string]$Override
    )
    if (Test-WingetId $Id) {
        Write-Output "Already installed: $Id"
        return
    }
    $arguments = @(
        'install', '--id', $Id, '-e',
        '--accept-package-agreements', '--accept-source-agreements'
    )
    if ($Override) {
        $arguments += @('--override', $Override)
    }
    & winget @arguments
    # 3010 means the installer succeeded and wants a reboot.
    if ($LASTEXITCODE -ne 0 -and $LASTEXITCODE -ne 3010) {
        throw "winget install $Id failed with exit $LASTEXITCODE"
    }
}

function Get-VcInstallPath {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio\Installer\vswhere.exe'
    if (-not (Test-Path $vswhere)) {
        return $null
    }
    $install = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if ([string]::IsNullOrWhiteSpace($install)) {
        return $null
    }
    return $install.Trim()
}

function Test-WebView2 {
    $guid = '{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
    $keys = @(
        "HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\$guid",
        "HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\$guid",
        "HKCU:\SOFTWARE\Microsoft\EdgeUpdate\Clients\$guid"
    )
    foreach ($key in $keys) {
        if (Test-Path $key) {
            $version = (Get-ItemProperty $key).pv
            if ($version -and $version -ne '0.0.0.0') {
                return $true
            }
        }
    }
    return $false
}

function Add-UserPathEntry {
    param([string]$Entry)
    if (-not (Test-Path $Entry)) {
        return
    }
    $current = [Environment]::GetEnvironmentVariable('Path', 'User')
    $parts = @()
    if ($current) {
        $parts = @($current.Split(';') | Where-Object { $_ -and ($_ -ne $Entry) })
    }
    $updated = (@($parts) + $Entry) -join ';'
    [Environment]::SetEnvironmentVariable('Path', $updated, 'User')
    if (($env:Path -split ';') -notcontains $Entry) {
        $env:Path = "$env:Path;$Entry"
    }
}

function Add-UserPathFront {
    param([string]$Entry)
    $current = [Environment]::GetEnvironmentVariable('Path', 'User')
    $parts = @()
    if ($current) {
        $parts = @($current.Split(';') | Where-Object { $_ -and ($_ -ine $Entry) })
    }
    $updated = (@($Entry) + $parts) -join ';'
    [Environment]::SetEnvironmentVariable('Path', $updated, 'User')
    $env:Path = "$Entry;" + (($env:Path -split ';' | Where-Object { $_ -and ($_ -ine $Entry) }) -join ';')
}

function Write-RunnerServicePath {
    param([string]$Root)
    $cargo = Join-Path $env:USERPROFILE '.cargo\bin'
    $parts = @($cargo)
    foreach ($entry in ($env:Path -split ';')) {
        if ($entry -and ($entry.TrimEnd('\') -ine $cargo.TrimEnd('\'))) {
            $parts += $entry
        }
    }
    $pathValue = $parts -join ';'
    $utf8 = New-Object System.Text.UTF8Encoding $false
    [System.IO.File]::WriteAllText((Join-Path $Root '.path'), $pathValue + "`n", $utf8)
    $envFile = Join-Path $Root '.env'
    $lines = @()
    if (Test-Path $envFile) {
        $lines = @(Get-Content -Path $envFile | Where-Object { $_ -notmatch '^(PATH|Path)=' })
    }
    $lines += "Path=$pathValue"
    [System.IO.File]::WriteAllLines($envFile, $lines, $utf8)
    Write-Output "Runner Path starts with $cargo."
}

function Assert-RustMsrv {
    $info = & rustc -vV | Out-String
    $release = ([regex]::Match($info, '(?m)^release: (\S+)')).Groups[1].Value
    if (-not $release -or ([version]$release -lt [version]$MinimumRust)) {
        throw "rustc ${release} is older than $MinimumRust."
    }
    Write-Output "rustc $release satisfies $MinimumRust."
}

function Get-RunnerAsset {
    $headers = @{ 'User-Agent' = 'newtype-runner-setup'; 'Accept' = 'application/vnd.github+json' }
    $release = Invoke-RestMethod -Headers $headers -Uri 'https://api.github.com/repos/actions/runner/releases/latest'
    $asset = $release.assets | Where-Object { $_.name -match '^actions-runner-win-x64-.+\.zip$' } | Select-Object -First 1
    $checksum = [regex]::Match([string]$release.body, '<!-- BEGIN SHA win-x64 -->([a-fA-F0-9]{64})<!-- END SHA win-x64 -->')
    if (-not $asset -or -not $checksum.Success) {
        throw "The latest actions/runner release has no win-x64 archive or checksum."
    }
    return @{ Name = $asset.name; Url = $asset.browser_download_url; Sha256 = $checksum.Groups[1].Value.ToLowerInvariant() }
}

function Install-RunnerPackage {
    param([string]$Destination)
    $asset = Get-RunnerAsset
    $zip = Join-Path ([System.IO.Path]::GetTempPath()) $asset.Name
    try {
        Invoke-WebRequest -Headers @{ 'User-Agent' = 'newtype-runner-setup' } -Uri $asset.Url -OutFile $zip
        $actual = (Get-FileHash -Algorithm SHA256 $zip).Hash.ToLowerInvariant()
        if ($asset.Sha256 -ne $actual) {
            throw "Runner archive checksum did not match $($asset.Name)."
        }
        New-Item -ItemType Directory -Force -Path $Destination | Out-Null
        Expand-Archive -Path $zip -DestinationPath $Destination -Force
        Write-Output "Installed runner files for $($asset.Name)."
    } finally {
        Remove-Item -Force -ErrorAction SilentlyContinue $zip
    }
}

function Get-LogonAccount {
    if ($env:USERDOMAIN -eq $env:COMPUTERNAME) {
        return ".\$env:USERNAME"
    }
    return "$env:USERDOMAIN\$env:USERNAME"
}

if (-not (Test-Administrator)) {
    throw "Run this script from an elevated PowerShell. Visual Studio Build Tools and the runner service need Administrator."
}
if (-not (Get-Command winget -ErrorAction SilentlyContinue)) {
    throw "winget is not on PATH."
}

$plainToken = $Token
$plainPassword = $null
try {
    if ($Uninstall) {
        if ([string]::IsNullOrWhiteSpace($plainToken)) {
            $plainToken = Read-Secret "Remove token"
        }
        if (Test-Path (Join-Path $RunnerRoot 'config.cmd')) {
            Push-Location $RunnerRoot
            try {
                if (Test-Path .\svc.cmd) {
                    & .\svc.cmd stop
                    & .\svc.cmd uninstall
                }
                & .\config.cmd remove --token $plainToken
                if ($LASTEXITCODE -ne 0) {
                    throw "config.cmd remove failed with exit $LASTEXITCODE"
                }
            } finally {
                Pop-Location
            }
            Remove-Item -Recurse -Force $RunnerRoot
        }
        Write-Output "Removed $RunnerName."
        return
    }

    if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
        Install-WingetId 'Git.Git'
    }
    $bash = @(
        (Join-Path $env:ProgramFiles 'Git\bin\bash.exe'),
        (Join-Path ${env:ProgramFiles(x86)} 'Git\bin\bash.exe')
    ) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -First 1
    if (-not $bash) {
        throw "Git for Windows bash.exe is required. Install Git and open a new elevated PowerShell."
    }
    Add-UserPathEntry (Split-Path $bash -Parent)

    if (-not (Get-Command node -ErrorAction SilentlyContinue) -or -not (Get-Command npm -ErrorAction SilentlyContinue)) {
        Install-WingetId 'OpenJS.NodeJS.LTS'
    }
    if (-not (Get-Command gh -ErrorAction SilentlyContinue)) {
        Install-WingetId 'GitHub.cli'
    }
    $cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
    if (-not (Get-Command rustup -ErrorAction SilentlyContinue) -and -not (Test-Path (Join-Path $cargoBin 'rustup.exe'))) {
        Install-WingetId 'Rustlang.Rustup'
    }
    Add-UserPathFront $cargoBin
    & rustup toolchain install stable --profile minimal
    & rustup default stable
    & rustup target add x86_64-pc-windows-msvc
    Assert-RustMsrv

    if (-not (Get-VcInstallPath)) {
        $override = '--wait --passive --norestart --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended'
        Install-WingetId 'Microsoft.VisualStudio.2022.BuildTools' $override
    }
    if (-not (Get-VcInstallPath)) {
        throw "MSVC C++ tools are still missing after the Visual Studio Build Tools install."
    }
    if (-not (Test-WebView2)) {
        Install-WingetId 'Microsoft.EdgeWebView2Runtime'
    }
    Write-Output "LLVM/clang is not required for this MSVC build; skipping."
    Write-Output "NSIS and WiX are not installed. The nsis bundle downloads NSIS during the build. This repo does not build an msi."

    if (-not (Test-Path (Join-Path $RunnerRoot 'config.cmd'))) {
        Install-RunnerPackage $RunnerRoot
    }
    Write-RunnerServicePath $RunnerRoot
    Push-Location $RunnerRoot
    try {
        if (-not (Test-Path .\.runner)) {
            if ([string]::IsNullOrWhiteSpace($plainToken)) {
                $plainToken = Read-Secret "Registration token"
            }
            $plainPassword = Read-Secret "Windows password for $(Get-LogonAccount) (service logon, not saved)"
            & .\config.cmd --unattended `
                --url $RepoUrl `
                --token $plainToken `
                --name $RunnerName `
                --labels newtype `
                --replace `
                --runasservice `
                --windowslogonaccount (Get-LogonAccount) `
                --windowslogonpassword $plainPassword
            if ($LASTEXITCODE -ne 0) {
                throw "config.cmd failed with exit $LASTEXITCODE. If the account cannot log on as a service, grant that right in secpol.msc and run this script again."
            }
        } else {
            Write-Output "Runner $RunnerName is already configured."
            & .\svc.cmd stop
            & .\svc.cmd start
        }
    } finally {
        Pop-Location
    }
    Write-Output "Runner $RunnerName is installed as $(Get-LogonAccount)."
} finally {
    $plainToken = $null
    $plainPassword = $null
    $Token = $null
}
