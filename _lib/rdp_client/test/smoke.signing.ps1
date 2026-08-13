[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
[Console]::OutputEncoding = New-Object Text.UTF8Encoding($false)
$OutputEncoding = New-Object Text.UTF8Encoding($false)

$CoreScript = Join-Path $PSScriptRoot '..\signing-core.ps1'
. $CoreScript
$ReportScript = Join-Path $PSScriptRoot '..\signing-report.ps1'
. $ReportScript

$Tokens = @(ConvertTo-RdpClientPublisherTokens `
    -Value 'sha1:OLD, sha256:ABC,sha256:abc,  sha384:KEEP  ')
if ($Tokens.Count -ne 4 -or $Tokens[1] -ne 'sha256:ABC') {
    throw 'Trusted publisher token parsing failed.'
}

$Added = @(Add-RdpClientPublisherToken `
    -Tokens $Tokens `
    -Token 'SHA256:ABC')
if ($Added.Count -ne 4) {
    throw 'Token addition should not append a duplicate owned value.'
}
if ($Added[0] -ne 'sha1:OLD' -or
    $Added[2] -ne 'sha256:abc' -or
    $Added[3] -ne 'sha384:KEEP') {
    throw 'Token addition should preserve all existing publisher entries and order.'
}

$Removed = @(Remove-RdpClientPublisherToken `
    -Tokens $Added `
    -Token 'sha256:abc')
if ($Removed.Count -ne 2 -or
    $Removed[0] -ne 'sha1:OLD' -or
    $Removed[1] -ne 'sha384:KEEP') {
    throw 'Token removal should remove only the owned entry.'
}

$Empty = @(ConvertTo-RdpClientPublisherTokens -Value '  ')
if ($Empty.Count -ne 0) {
    throw 'An empty policy value should produce no tokens.'
}
$AddedToEmpty = @(
    Add-RdpClientPublisherToken -Tokens @() -Token 'sha256:FIRST'
)
if ($AddedToEmpty.Count -ne 1 -or $AddedToEmpty[0] -ne 'sha256:FIRST') {
    throw 'Token addition to an empty policy failed.'
}

$Configuration = Get-RdpClientSigningConfiguration
if ($Configuration.CertificateStore -ne 'Cert:\CurrentUser\My' -or
    $Configuration.TrustCertificateStore -ne 'Cert:\CurrentUser\Root' -or
    $Configuration.TrustStoreName -ne 'Root' -or
    $Configuration.TrustStoreLocation -ne 'CurrentUser' -or
    $Configuration.PolicyValueName -ne 'TrustedCertThumbprints') {
    throw 'Signing configuration uses an unexpected certificate store or policy value.'
}
if ($null -ne $Configuration.PSObject.Properties['StateKeyPath']) {
    throw 'Signing configuration should not create a separate ownership registry record.'
}
if (-not $Configuration.PrivateKeyFriendlyName.Contains('PRIVATE KEY') -or
    -not $Configuration.PrivateKeyFriendlyName.Contains('CurrentUser\Root') -or
    -not $Configuration.RootTrustFriendlyName.Contains('TRUST COPY') -or
    -not $Configuration.RootTrustFriendlyName.Contains('CurrentUser\My') -or
    -not $Configuration.PrivateKeyFriendlyName.Contains('.sign remove') -or
    -not $Configuration.RootTrustFriendlyName.Contains('TrustedCertThumbprints')) {
    throw 'Signing certificate labels should describe their role and paired cleanup locations.'
}

$MissingNotice = Write-RdpClientSigningMissingNotice `
    -CommandName 'rdp.test' 6>&1 | Out-String
foreach ($Expected in @(
    '[RDP] Signing:   not installed; file remains unsigned.',
    'Run "rdp.test .sign install" to enable trusted signing.'
)) {
    if (-not $MissingNotice.Contains($Expected)) {
        throw "Missing signing notice is missing '$Expected'.`n$MissingNotice"
    }
}
$CachedMissingNotice = Write-RdpClientSigningMissingNotice `
    -CommandName 'rdp.test' `
    -Cached 6>&1 | Out-String
foreach ($Expected in @(
    '[RDP] Signing:   not installed; cached file remains unsigned.',
    'Run "rdp.test .sign install" to enable trusted signing.'
)) {
    if (-not $CachedMissingNotice.Contains($Expected)) {
        throw "Cached signing notice is missing '$Expected'.`n$CachedMissingNotice"
    }
}

$TestRsa = [Security.Cryptography.RSA]::Create(2048)
try {
    $TestRequest = [Security.Cryptography.X509Certificates.CertificateRequest]::new(
        'CN=rdp-client-signing-smoke',
        $TestRsa,
        [Security.Cryptography.HashAlgorithmName]::SHA256,
        [Security.Cryptography.RSASignaturePadding]::Pkcs1
    )
    $TestCertificate = $TestRequest.CreateSelfSigned(
        [DateTimeOffset]::Now.AddMinutes(-1),
        [DateTimeOffset]::Now.AddMinutes(5)
    )
    try {
        $Sha256Fingerprint = Get-RdpClientCertificateFingerprintSha256 `
            -Certificate $TestCertificate
        if ($TestCertificate.Thumbprint.Length -ne 40 -or
            $Sha256Fingerprint.Length -ne 64 -or
            $Sha256Fingerprint -eq $TestCertificate.Thumbprint) {
            throw 'Certificate SHA-1 and SHA-256 identifiers were not calculated independently.'
        }

        $script:RdpSignMockMode = 'Sha256Success'
        $script:RdpSignMockCalls = New-Object 'Collections.Generic.List[string]'
        function Invoke-RdpClientRdpSignProcess {
            param(
                [Parameter(Mandatory = $true)][string]$Path,
                [Parameter(Mandatory = $true)][string]$CertificateHash
            )

            $null = $Path
            $script:RdpSignMockCalls.Add($CertificateHash)
            if ($script:RdpSignMockMode -eq 'Sha256Success') {
                return [pscustomobject]@{ ExitCode = 0; Output = '' }
            }
            if ($script:RdpSignMockMode -eq 'FallbackSuccess') {
                if ($CertificateHash -eq $Sha256Fingerprint) {
                    return [pscustomobject]@{
                        ExitCode = 0x80092004
                        Output   = 'Unable to locate the certificate specified.'
                    }
                }
                return [pscustomobject]@{ ExitCode = 0; Output = '' }
            }
            return [pscustomobject]@{
                ExitCode = 5
                Output   = 'Access is denied.'
            }
        }

        $null = Invoke-RdpClientRdpSignCompatible `
            -Path 'ignored.rdp' `
            -Certificate $TestCertificate
        if ($script:RdpSignMockCalls.Count -ne 1 -or
            $script:RdpSignMockCalls[0] -ne $Sha256Fingerprint) {
            throw 'rdpsign compatibility should prefer the documented SHA-256 fingerprint.'
        }

        $script:RdpSignMockMode = 'FallbackSuccess'
        $script:RdpSignMockCalls.Clear()
        $null = Invoke-RdpClientRdpSignCompatible `
            -Path 'ignored.rdp' `
            -Certificate $TestCertificate
        if ($script:RdpSignMockCalls.Count -ne 2 -or
            $script:RdpSignMockCalls[0] -ne $Sha256Fingerprint -or
            $script:RdpSignMockCalls[1] -ne $TestCertificate.Thumbprint) {
            throw 'rdpsign compatibility should retry SHA-1 only after certificate lookup fails.'
        }

        $script:RdpSignMockMode = 'FatalFailure'
        $script:RdpSignMockCalls.Clear()
        $FatalFailure = $null
        try {
            $null = Invoke-RdpClientRdpSignCompatible `
                -Path 'ignored.rdp' `
                -Certificate $TestCertificate
        } catch {
            $FatalFailure = $_.Exception.Message
        }
        if ($script:RdpSignMockCalls.Count -ne 1 -or
            [string]::IsNullOrWhiteSpace($FatalFailure) -or
            -not $FatalFailure.Contains('Access is denied.')) {
            throw 'rdpsign compatibility must not hide or retry unrelated signing failures.'
        }
    } finally {
        $TestCertificate.Dispose()
    }
} finally {
    $TestRsa.Dispose()
}

$LabelState = [pscustomobject]@{
    OwnedCertificates = @([pscustomobject]@{
        Thumbprint = 'PRIVATE'; FriendlyName = 'old private label'
    })
    TrustCertificates = @([pscustomobject]@{
        Thumbprint = 'ROOT'; FriendlyName = 'old root label'
    })
}
$LabelChanges = @(Get-RdpClientSigningFriendlyNameChanges `
    -State $LabelState `
    -Configuration $Configuration)
if ($LabelChanges.Count -ne 2 -or
    $LabelChanges[0].FriendlyName -ne $Configuration.PrivateKeyFriendlyName -or
    $LabelChanges[1].FriendlyName -ne $Configuration.RootTrustFriendlyName) {
    throw 'Signing install should repair both certificate-store friendly names.'
}

$RdpSignPath = Get-RdpClientRdpSignPath
if (-not [IO.File]::Exists($RdpSignPath)) {
    throw "rdpsign.exe not found: $RdpSignPath"
}
$SystemDirectory = Get-RdpClientSigningSystemDirectory
foreach ($GuiComponent in @('mmc.exe', 'certmgr.msc')) {
    $GuiPath = Join-Path $SystemDirectory $GuiComponent
    if (-not [IO.File]::Exists($GuiPath)) {
        throw "Certificate manager component not found: $GuiPath"
    }
}

$MissingState = [pscustomobject]@{
    Name              = 'Missing'
    Certificate       = $null
    OwnedCertificates = @()
    TrustCertificates = @()
    Reason            = 'Not installed.'
}
$MissingStatusOutput = Write-RdpClientSigningStatus `
    -State $MissingState `
    -Configuration $Configuration 6>&1 | Out-String
foreach ($Expected in @(
    '[RDP] Signing publisher: Not installed',
    'ABSENT  Cert:\CurrentUser\My\<swaw-kit-publisher>',
    'ABSENT  Cert:\CurrentUser\Root\<swaw-kit-publisher>'
)) {
    if (-not $MissingStatusOutput.Contains($Expected)) {
        throw "Missing signing status output is missing '$Expected'.`n$MissingStatusOutput"
    }
}
$MissingPolicyIsAbsent = $MissingStatusOutput.Contains(
    'ABSENT  HKCU:\Software\Policies\Microsoft\Windows NT\Terminal Services\TrustedCertThumbprints [swaw-kit-publisher]'
)
$MissingPolicyIsShared = $MissingStatusOutput -match (
    'SHARED\s+HKCU:\\Software\\Policies\\Microsoft\\Windows NT\\' +
    'Terminal Services\\TrustedCertThumbprints = \d+ opaque fingerprint\(s\); ' +
    'no swaw-kit certificate remains to match'
)
if (-not $MissingPolicyIsAbsent -and -not $MissingPolicyIsShared) {
    throw "Missing signing status should report the policy as ABSENT or SHARED.`n$MissingStatusOutput"
}
foreach ($Unexpected in @('Signing publisher: Missing', 'UNKNOWN', 'N/A', 'DETAIL')) {
    if ($MissingStatusOutput.Contains($Unexpected)) {
        throw "Normal uninstalled status should not contain '$Unexpected'.`n$MissingStatusOutput"
    }
}

$DryRunOutput = Write-RdpClientSigningInstallChanges `
    -State $MissingState `
    -Configuration $Configuration `
    -DryRun 6>&1 | Out-String
foreach ($Expected in @(
    '[RDP] Signing install --dry-run',
    'ADD     Cert:\CurrentUser\My\<generated-thumbprint>',
    'ADD     Cert:\CurrentUser\Root\<same-generated-thumbprint>',
    'PRIVATE KEY; paired: CurrentUser\Root + TrustedCertThumbprints',
    'TRUST COPY; paired: CurrentUser\My + TrustedCertThumbprints',
    'TrustedCertThumbprints += sha256:<generated-fingerprint>'
)) {
    if (-not $DryRunOutput.Contains($Expected)) {
        throw "Signing dry-run output is missing '$Expected'.`n$DryRunOutput"
    }
}
if ($DryRunOutput.Contains('HKCU:\Software\swaw-kit\rdp-client\signing')) {
    throw "Signing dry-run should not advertise an ownership registry record.`n$DryRunOutput"
}

$SigningScript = [IO.File]::ReadAllText(
    (Join-Path $PSScriptRoot '..\signing.ps1'),
    [Text.Encoding]::UTF8
)
if (-not $SigningScript.Contains(
    '$PublicCertificate.FriendlyName = $Configuration.RootTrustFriendlyName'
)) {
    throw 'The Root trust copy should receive its own maintenance label before import.'
}
$RemoveStart = $SigningScript.IndexOf('function Remove-RdpClientSigning')
$RemoveEnd = $SigningScript.IndexOf('function Open-RdpClientCertificateManager')
$RemoveBody = $SigningScript.Substring($RemoveStart, $RemoveEnd - $RemoveStart)
$PolicyRemove = $RemoveBody.IndexOf(
    'Set-RdpClientTrustedPublisherTokens -Tokens $Tokens'
)
$RootRemove = $RemoveBody.IndexOf('Remove-RdpClientRootTrustCertificates')
$PrivateRemove = $RemoveBody.IndexOf('Remove-Item -LiteralPath')
if ($PolicyRemove -lt 0 -or $RootRemove -le $PolicyRemove -or
    $PrivateRemove -le $RootRemove) {
    throw 'Signing removal must delete policy tokens, then Root trust, then My certificates.'
}

Write-Host 'rdp client signing tests: PASS' -ForegroundColor Green
