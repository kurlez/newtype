# Install or remove the newtype Windows x64 GitHub Actions runner.
# The registration token is a parameter or a hidden prompt. It is not written to disk.
[CmdletBinding()]
param(
    [string]$Token,
    [string]$RemoveToken,
    [switch]$Uninstall,
    [string]$RunnerRoot = (Join-Path $env:USERPROFILE 'actions-runners\newtype-win'),
    [string]$LogonAccount,
    [pscredential]$Credential
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
$DefaultLogonAccount = 'NT AUTHORITY\SYSTEM'

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

function Test-PathUnderRoot {
    param([string]$Entry, [string]$Root)
    if (-not $Entry -or -not $Root) { return $false }
    $left = $Entry.TrimEnd('\')
    $right = $Root.TrimEnd('\')
    return $left.Equals($right, [StringComparison]::OrdinalIgnoreCase) -or
        $left.StartsWith($right + '\', [StringComparison]::OrdinalIgnoreCase)
}

function Initialize-VerifiedRustup {
    $profileCargo = Join-Path $env:USERPROFILE '.cargo'
    $profileRustup = Join-Path $env:USERPROFILE '.rustup'
    $profileExe = Join-Path $profileCargo 'bin\rustup.exe'
    $seenCargo = $env:CARGO_HOME
    $seenRustup = $env:RUSTUP_HOME
    $cargoHome = $profileCargo
    $rustupHome = $profileRustup
    if (-not (Test-Path $profileExe) -and $seenCargo -and (Test-Path (Join-Path $seenCargo 'bin\rustup.exe'))) {
        $cargoHome = $seenCargo
        if ($seenRustup -and (Test-Path $seenRustup)) {
            $rustupHome = $seenRustup
        }
    }
    $script:StaleRustRoots = @()
    foreach ($root in @($seenCargo, $seenRustup)) {
        if ($root -and -not (Test-PathUnderRoot $root $cargoHome) -and -not (Test-PathUnderRoot $root $rustupHome)) {
            $script:StaleRustRoots += $root
        }
    }
    $env:CARGO_HOME = $cargoHome
    $env:RUSTUP_HOME = $rustupHome
    $cargoBin = Join-Path $cargoHome 'bin'
    $kept = @($cargoBin)
    foreach ($entry in ($env:Path -split ';')) {
        if (-not $entry) { continue }
        if (Test-PathUnderRoot $entry $cargoBin) { continue }
        $stale = $false
        foreach ($root in $script:StaleRustRoots) {
            if (Test-PathUnderRoot $entry $root) { $stale = $true; break }
        }
        if (-not $stale) { $kept += $entry }
    }
    $env:Path = $kept -join ';'
    if ($script:StaleRustRoots.Count -gt 0) {
        Write-Output "Ignoring stale CARGO_HOME/RUSTUP_HOME ($($script:StaleRustRoots -join ', ')). Using $cargoHome."
    }
    return @{ CargoHome = $cargoHome; RustupHome = $rustupHome; CargoBin = $cargoBin }
}

function Write-RunnerServicePath {
    param(
        [string]$Root,
        [string]$CargoHome,
        [string]$RustupHome
    )
    $cargoBin = Join-Path $CargoHome 'bin'
    $parts = @($cargoBin)
    foreach ($entry in ($env:Path -split ';')) {
        if (-not $entry) { continue }
        if (Test-PathUnderRoot $entry $cargoBin) { continue }
        $stale = $false
        foreach ($root in $script:StaleRustRoots) {
            if (Test-PathUnderRoot $entry $root) { $stale = $true; break }
        }
        if (-not $stale) { $parts += $entry }
    }
    $pathValue = $parts -join ';'
    $utf8 = New-Object System.Text.UTF8Encoding $false
    [System.IO.File]::WriteAllText((Join-Path $Root '.path'), $pathValue + "`n", $utf8)
    $envFile = Join-Path $Root '.env'
    $lines = @()
    if (Test-Path $envFile) {
        $lines = @(Get-Content -Path $envFile | Where-Object { $_ -notmatch '^(PATH|Path|CARGO_HOME|RUSTUP_HOME)=' })
    }
    $lines += "CARGO_HOME=$CargoHome"
    $lines += "RUSTUP_HOME=$RustupHome"
    $lines += "Path=$pathValue"
    [System.IO.File]::WriteAllLines($envFile, $lines, $utf8)
    Write-Output "Runner uses CARGO_HOME=$CargoHome and RUSTUP_HOME=$RustupHome."
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

function Test-WellKnownServiceAccount {
    param([string]$Account)
    try {
        $nt = New-Object System.Security.Principal.NTAccount($Account)
        $sid = $nt.Translate([System.Security.Principal.SecurityIdentifier]).Value
    } catch {
        return $false
    }
    return $sid -in @('S-1-5-18', 'S-1-5-19', 'S-1-5-20')
}

function Get-RunnerWindowsService {
    param([string]$Root)
    $services = @(Get-Service -ErrorAction SilentlyContinue | Where-Object { $_.Name -like 'actions.runner.*' })
    $serviceFile = Join-Path $Root '.service'
    if (Test-Path $serviceFile) {
        $name = (Get-Content -Raw $serviceFile).Trim()
        $match = $services | Where-Object { $_.Name -eq $name } | Select-Object -First 1
        if ($match) { return $match }
    }
    return $services | Where-Object { $_.Name -like '*newtype-win*' } | Select-Object -First 1
}

function Invoke-RunnerService {
    param($Service)
    Set-Service -Name $Service.Name -StartupType Automatic
    $current = Get-Service -Name $Service.Name
    if ($current.Status -eq 'Running') {
        Restart-Service -Name $Service.Name -Force
    } else {
        Start-Service -Name $Service.Name
    }
    Write-Output "Service $($Service.Name) is Automatic."
}

function Add-RunnerDirectoryAccess {
    param([string]$Account, [string[]]$Paths)
    foreach ($path in $Paths) {
        if (-not $path) { continue }
        if (-not (Test-Path $path)) {
            New-Item -ItemType Directory -Force -Path $path | Out-Null
        }
        & icacls.exe $path /grant "${Account}:(OI)(CI)M" /C | Out-Null
        if ($LASTEXITCODE -ne 0) {
            throw "Could not grant modify access on $path to $Account."
        }
    }
}

function Get-GitHubRunnerToken {
    param([ValidateSet('registration-token', 'remove-token')][string]$Kind)
    if (-not (Get-Command gh -ErrorAction SilentlyContinue)) {
        return $null
    }
    $output = & gh api -X POST "repos/kurlez/newtype/actions/runners/$Kind" --jq .token 2>$null
    if ($LASTEXITCODE -ne 0 -or -not $output) {
        return $null
    }
    $text = if ($output -is [array]) { [string]$output[-1] } else { [string]$output }
    $text = $text.Trim()
    if ([string]::IsNullOrWhiteSpace($text)) {
        return $null
    }
    return $text
}

function Resolve-RunnerToken {
    param([string]$Provided, [string]$Kind, [string]$PromptLabel)
    if (-not [string]::IsNullOrWhiteSpace($Provided)) {
        return $Provided.Trim()
    }
    $minted = Get-GitHubRunnerToken $Kind
    if ($minted) {
        return $minted
    }
    return Read-Secret $PromptLabel
}

if (-not (Test-Administrator)) {
    throw "Run this script from an elevated PowerShell. Visual Studio Build Tools and the runner service need Administrator."
}
if (-not (Get-Command winget -ErrorAction SilentlyContinue)) {
    throw "winget is not on PATH."
}

$plainToken = $null
$plainRemove = $null
$plainPassword = $null
try {
    if ($Uninstall) {
        $plainRemove = Resolve-RunnerToken $RemoveToken 'remove-token' 'Remove token'
        $service = Get-RunnerWindowsService $RunnerRoot
        if ($service) {
            Stop-Service -Name $service.Name -Force -ErrorAction SilentlyContinue
        }
        if (Test-Path (Join-Path $RunnerRoot 'config.cmd')) {
            Push-Location $RunnerRoot
            try {
                & .\config.cmd remove --token $plainRemove
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
    $rust = Initialize-VerifiedRustup
    if (-not (Test-Path (Join-Path $rust.CargoBin 'rustup.exe'))) {
        Install-WingetId 'Rustlang.Rustup'
        $rust = Initialize-VerifiedRustup
    }
    if (-not (Test-Path (Join-Path $rust.CargoBin 'rustup.exe'))) {
        throw "rustup.exe was not found under $($rust.CargoHome). A CARGO_HOME value that does not contain rustup is ignored."
    }
    Add-UserPathFront $rust.CargoBin
    $rustupExe = Join-Path $rust.CargoBin 'rustup.exe'
    & $rustupExe toolchain install stable --profile minimal
    & $rustupExe default stable
    & $rustupExe target add x86_64-pc-windows-msvc
    $env:Path = "$($rust.CargoBin);$env:Path"
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

    $serviceAccount = $DefaultLogonAccount
    if ($Credential) {
        $serviceAccount = $Credential.UserName
        if (-not (Test-WellKnownServiceAccount $serviceAccount)) {
            $plainPassword = $Credential.GetNetworkCredential().Password
        }
    } elseif (-not [string]::IsNullOrWhiteSpace($LogonAccount)) {
        $serviceAccount = $LogonAccount
    }
    if (-not (Test-WellKnownServiceAccount $serviceAccount) -and [string]::IsNullOrWhiteSpace($plainPassword)) {
        throw "Pass -Credential for $serviceAccount. This script does not prompt for a Windows password."
    }

    if (-not (Test-Path (Join-Path $RunnerRoot 'config.cmd'))) {
        Install-RunnerPackage $RunnerRoot
    }
    Write-RunnerServicePath $RunnerRoot $rust.CargoHome $rust.RustupHome
    Add-RunnerDirectoryAccess $serviceAccount @($RunnerRoot, $rust.CargoHome, $rust.RustupHome)
    Push-Location $RunnerRoot
    try {
        $needsConfigure = -not (Test-Path .\.runner)
        if ((Test-Path .\.runner) -and -not (Get-RunnerWindowsService $RunnerRoot)) {
            Write-Output "Runner $RunnerName is configured but has no actions.runner service. Removing the registration and configuring it again."
            $plainRemove = Resolve-RunnerToken $RemoveToken 'remove-token' 'Remove token'
            & .\config.cmd remove --token $plainRemove
            if ($LASTEXITCODE -ne 0) {
                throw "config.cmd remove failed with exit $LASTEXITCODE"
            }
            $plainRemove = $null
            $needsConfigure = $true
        }
        if ($needsConfigure) {
            $plainToken = Resolve-RunnerToken $Token 'registration-token' 'Registration token'
            $configArgs = @(
                '--unattended',
                '--url', $RepoUrl,
                '--token', $plainToken,
                '--name', $RunnerName,
                '--labels', 'newtype',
                '--replace',
                '--runasservice',
                '--windowslogonaccount', $serviceAccount
            )
            if (-not [string]::IsNullOrWhiteSpace($plainPassword)) {
                $configArgs += @('--windowslogonpassword', $plainPassword)
            }
            & .\config.cmd @configArgs
            if ($LASTEXITCODE -ne 0) {
                throw "config.cmd failed with exit $LASTEXITCODE"
            }
            $plainToken = $null
        } else {
            Write-Output "Runner $RunnerName is already configured."
        }
        $service = Get-RunnerWindowsService $RunnerRoot
        if (-not $service) {
            throw "config.cmd did not create an actions.runner service."
        }
        Invoke-RunnerService $service
    } finally {
        Pop-Location
    }
    Write-Output "Runner $RunnerName service runs as $serviceAccount."
} finally {
    $plainToken = $null
    $plainRemove = $null
    $plainPassword = $null
    $Token = $null
    $RemoveToken = $null
}
