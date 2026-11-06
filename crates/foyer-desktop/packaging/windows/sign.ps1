# Signs one Windows file (Foyer.exe, the installer or its uninstaller).
#
#   pwsh -File sign.ps1 path\to\file.exe
#
# Uses whichever signing setup is present in the environment:
#
# 1. Azure Trusted Signing (Artifact Signing): AZURE_TENANT_ID,
#    AZURE_CLIENT_ID, AZURE_CLIENT_SECRET, TRUSTED_SIGNING_ENDPOINT
#    (e.g. https://weu.codesigning.azure.net), TRUSTED_SIGNING_ACCOUNT and
#    TRUSTED_SIGNING_PROFILE.
# 2. A code signing certificate as a .pfx: WINDOWS_CERTIFICATE (base64 of the
#    .pfx) and WINDOWS_CERTIFICATE_PASSWORD.
#
# With neither it does nothing, so unsigned builds still work. Files are
# timestamped, so signatures stay valid after the certificate expires.

param([Parameter(Mandatory = $true)][string]$File)

$ErrorActionPreference = 'Stop'

function Find-SignTool {
    $kits = Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'
    $tool = Get-ChildItem -Path $kits -Recurse -Filter signtool.exe -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match '\\x64\\' } |
        Sort-Object FullName -Descending |
        Select-Object -First 1
    if (-not $tool) { throw 'signtool.exe not found; install the Windows SDK' }
    return $tool.FullName
}

if ($env:TRUSTED_SIGNING_ACCOUNT) {
    $signtool = Find-SignTool
    $work = Join-Path $env:RUNNER_TEMP 'trusted-signing'
    $dlib = Get-ChildItem -Path $work -Recurse -Filter Azure.CodeSigning.Dlib.dll -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match '\\x64\\' } |
        Select-Object -First 1
    if (-not $dlib) {
        New-Item -ItemType Directory -Force -Path $work | Out-Null
        $nupkg = Join-Path $work 'client.zip'
        Invoke-WebRequest -Uri 'https://www.nuget.org/api/v2/package/Microsoft.Trusted.Signing.Client' -OutFile $nupkg
        Expand-Archive -Path $nupkg -DestinationPath (Join-Path $work 'client') -Force
        $dlib = Get-ChildItem -Path $work -Recurse -Filter Azure.CodeSigning.Dlib.dll |
            Where-Object { $_.FullName -match '\\x64\\' } |
            Select-Object -First 1
    }
    $metadata = Join-Path $work 'metadata.json'
    @{
        Endpoint               = $env:TRUSTED_SIGNING_ENDPOINT
        CodeSigningAccountName = $env:TRUSTED_SIGNING_ACCOUNT
        CertificateProfileName = $env:TRUSTED_SIGNING_PROFILE
    } | ConvertTo-Json | Set-Content -Path $metadata -Encoding utf8
    & $signtool sign /v /fd SHA256 /tr 'http://timestamp.acs.microsoft.com' /td SHA256 `
        /dlib $dlib.FullName /dmdf $metadata $File
    if ($LASTEXITCODE -ne 0) { throw "signtool failed for $File" }
}
elseif ($env:WINDOWS_CERTIFICATE) {
    $signtool = Find-SignTool
    $pfx = Join-Path $env:RUNNER_TEMP 'foyer-signing.pfx'
    [IO.File]::WriteAllBytes($pfx, [Convert]::FromBase64String($env:WINDOWS_CERTIFICATE))
    try {
        & $signtool sign /v /fd SHA256 /tr 'http://timestamp.digicert.com' /td SHA256 `
            /f $pfx /p $env:WINDOWS_CERTIFICATE_PASSWORD /d 'Foyer' /du 'https://foyer.lol' $File
        if ($LASTEXITCODE -ne 0) { throw "signtool failed for $File" }
    }
    finally {
        Remove-Item -Force $pfx -ErrorAction SilentlyContinue
    }
}
else {
    Write-Host "No signing setup in the environment; leaving $File unsigned."
}
