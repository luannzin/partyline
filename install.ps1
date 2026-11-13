# PartyLine for Windows: downloads the latest release, puts it on your PATH and starts it.
#   irm https://raw.githubusercontent.com/luannzin/partyline/main/install.ps1 | iex
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'  # the progress bar makes downloads crawl in Windows PowerShell 5.1
[Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12

$dir = Join-Path $env:LOCALAPPDATA 'Programs\PartyLine'
$exe = Join-Path $dir 'partyline.exe'
New-Item -ItemType Directory -Force $dir | Out-Null
Write-Host 'Downloading PartyLine...'
Invoke-WebRequest 'https://github.com/luannzin/partyline/releases/latest/download/partyline-windows-x86_64.exe' -OutFile "$exe.part"
Move-Item -Force "$exe.part" $exe

$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if (-not (($userPath -split ';') -contains $dir)) {
    [Environment]::SetEnvironmentVariable('Path', ((@($userPath, $dir) | Where-Object { $_ }) -join ';'), 'User')
    $env:Path += ";$dir"
}
Write-Host "Installed to $exe. It keeps itself up to date. Next time just run: partyline"
& $exe
