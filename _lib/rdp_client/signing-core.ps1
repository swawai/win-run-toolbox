Set-StrictMode -Version 2.0

function Get-RdpClientSigningConfiguration {
    return [pscustomobject]@{
        Subject                = 'CN=swaw-kit RDP File Publisher'
        FriendlyName           = 'swaw-kit RDP File Publisher'
        PrivateKeyFriendlyName = 'swaw-kit RDP Publisher [PRIVATE KEY; paired: CurrentUser\Root + TrustedCertThumbprints; remove via .sign remove]'
        RootTrustFriendlyName  = 'swaw-kit RDP Publisher [TRUST COPY; paired: CurrentUser\My + TrustedCertThumbprints; remove via .sign remove]'
        CertificateStore       = 'Cert:\CurrentUser\My'
        TrustCertificateStore = 'Cert:\CurrentUser\Root'
        TrustStoreName         = 'Root'
        TrustStoreLocation     = 'CurrentUser'
        PolicyKeyPath    = 'HKCU:\Software\Policies\Microsoft\Windows NT\Terminal Services'
        PolicyValueName  = 'TrustedCertThumbprints'
    }
}

function Get-RdpClientCertificateFingerprintSha256 {
    param(
        [Parameter(Mandatory = $true)]
        [Security.Cryptography.X509Certificates.X509Certificate2]$Certificate
    )

    $Algorithm = [Security.Cryptography.SHA256]::Create()
    try {
        $Bytes = $Algorithm.ComputeHash($Certificate.RawData)
        return ([BitConverter]::ToString($Bytes)).Replace('-', '')
    } finally {
        $Algorithm.Dispose()
    }
}

function Get-RdpClientSigningSystemDirectory {
    $WindowsDirectory = [Environment]::GetFolderPath(
        [Environment+SpecialFolder]::Windows
    )
    if ([Environment]::Is64BitOperatingSystem -and
        -not [Environment]::Is64BitProcess) {
        return Join-Path $WindowsDirectory 'Sysnative'
    }
    return Join-Path $WindowsDirectory 'System32'
}

function Get-RdpClientRdpSignPath {
    return Join-Path (Get-RdpClientSigningSystemDirectory) 'rdpsign.exe'
}

function Get-RdpClientRegistryValueState {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$Name
    )

    if (-not (Test-Path -LiteralPath $Path)) {
        return [pscustomobject]@{ Exists = $false; Value = $null }
    }
    $Item = Get-ItemProperty -LiteralPath $Path
    $Property = $Item.PSObject.Properties[$Name]
    if ($null -eq $Property) {
        return [pscustomobject]@{ Exists = $false; Value = $null }
    }
    return [pscustomobject]@{ Exists = $true; Value = $Property.Value }
}

function ConvertTo-RdpClientPublisherTokens {
    param([AllowNull()][AllowEmptyString()][string]$Value)

    if ([string]::IsNullOrWhiteSpace($Value)) {
        return [string[]]@()
    }
    return [string[]]@(
        $Value -split ',' |
            ForEach-Object { $_.Trim() } |
            Where-Object { $_.Length -gt 0 }
    )
}

function Add-RdpClientPublisherToken {
    param(
        [AllowEmptyCollection()][string[]]$Tokens,
        [Parameter(Mandatory = $true)][string]$Token
    )

    foreach ($Item in @($Tokens)) {
        if ([string]::Equals(
            $Item,
            $Token,
            [StringComparison]::OrdinalIgnoreCase
        )) {
            return [string[]]@($Tokens)
        }
    }
    return [string[]]@($Tokens + @($Token))
}

function Remove-RdpClientPublisherToken {
    param(
        [AllowEmptyCollection()][string[]]$Tokens,
        [Parameter(Mandatory = $true)][string]$Token
    )

    return [string[]]@($Tokens | Where-Object {
        -not [string]::Equals($_, $Token, [StringComparison]::OrdinalIgnoreCase)
    })
}

function Get-RdpClientTrustedPublisherTokens {
    param([pscustomobject]$Configuration = (Get-RdpClientSigningConfiguration))

    $ValueState = Get-RdpClientRegistryValueState `
        -Path $Configuration.PolicyKeyPath `
        -Name $Configuration.PolicyValueName
    return ConvertTo-RdpClientPublisherTokens -Value $ValueState.Value
}

function Set-RdpClientTrustedPublisherTokens {
    param(
        [AllowEmptyCollection()][string[]]$Tokens,
        [pscustomobject]$Configuration = (Get-RdpClientSigningConfiguration)
    )

    if (@($Tokens).Count -eq 0) {
        if (Test-Path -LiteralPath $Configuration.PolicyKeyPath) {
            Remove-ItemProperty `
                -LiteralPath $Configuration.PolicyKeyPath `
                -Name $Configuration.PolicyValueName `
                -ErrorAction SilentlyContinue
        }
        return
    }

    New-Item -Path $Configuration.PolicyKeyPath -Force | Out-Null
    New-ItemProperty `
        -LiteralPath $Configuration.PolicyKeyPath `
        -Name $Configuration.PolicyValueName `
        -Value ([string]::Join(',', [string[]]$Tokens)) `
        -PropertyType String `
        -Force | Out-Null
}

function Get-RdpClientOwnedPublisherCertificates {
    param([pscustomobject]$Configuration = (Get-RdpClientSigningConfiguration))

    return @(
        Get-ChildItem -Path $Configuration.CertificateStore |
            Where-Object {
                $_.Subject -eq $Configuration.Subject
            }
    )
}

function Get-RdpClientOwnedRootTrustCertificates {
    param([pscustomobject]$Configuration = (Get-RdpClientSigningConfiguration))

    return @(
        Get-ChildItem -Path $Configuration.TrustCertificateStore |
            Where-Object { $_.Subject -eq $Configuration.Subject }
    )
}

function Test-RdpClientCodeSigningUsage {
    param(
        [Parameter(Mandatory = $true)]
        [Security.Cryptography.X509Certificates.X509Certificate2]$Certificate
    )

    $UsageExtension = @($Certificate.Extensions | Where-Object {
        $_.Oid.Value -eq '2.5.29.37'
    } | Select-Object -First 1)
    if ($UsageExtension.Count -ne 1) {
        return $false
    }
    return @($UsageExtension[0].EnhancedKeyUsages | Where-Object {
        $_.Value -eq '1.3.6.1.5.5.7.3.3'
    }).Count -eq 1
}

function Get-RdpClientSigningState {
    param([pscustomobject]$Configuration = (Get-RdpClientSigningConfiguration))

    $OwnedCertificates = @(Get-RdpClientOwnedPublisherCertificates -Configuration $Configuration)
    $TrustCertificates = @(Get-RdpClientOwnedRootTrustCertificates -Configuration $Configuration)
    if ($OwnedCertificates.Count -eq 0) {
        if ($TrustCertificates.Count -eq 0) {
            return [pscustomobject]@{
                Name              = 'Missing'
                Reason            = ''
                Certificate       = $null
                OwnedCertificates = $OwnedCertificates
                TrustCertificates = $TrustCertificates
                PolicyToken       = $null
            }
        }
        return [pscustomobject]@{
            Name              = 'PrivateKeyMissing'
            Reason            = 'A matching Root trust certificate exists, but the publisher certificate and private key are absent from CurrentUser\My.'
            Certificate       = $null
            OwnedCertificates = $OwnedCertificates
            TrustCertificates = $TrustCertificates
            PolicyToken       = $null
        }
    }
    if ($OwnedCertificates.Count -ne 1) {
        return [pscustomobject]@{
            Name              = 'CertificateConflict'
            Reason            = 'Multiple certificates use the reserved swaw-kit RDP publisher subject in CurrentUser\My.'
            Certificate       = $null
            OwnedCertificates = $OwnedCertificates
            TrustCertificates = $TrustCertificates
            PolicyToken       = $null
        }
    }

    $Certificate = $OwnedCertificates[0]
    $ActualFingerprint = Get-RdpClientCertificateFingerprintSha256 -Certificate $Certificate
    $PolicyToken = 'sha256:' + $ActualFingerprint
    if (-not $Certificate.HasPrivateKey) {
        return [pscustomobject]@{
            Name = 'PrivateKeyMissing'; Reason = 'The publisher certificate has no private key.'
            Certificate = $Certificate; OwnedCertificates = $OwnedCertificates
            TrustCertificates = $TrustCertificates; PolicyToken = $PolicyToken
        }
    }
    if (-not (Test-RdpClientCodeSigningUsage -Certificate $Certificate)) {
        return [pscustomobject]@{
            Name = 'UsageMismatch'; Reason = 'The publisher certificate is not valid for code signing.'
            Certificate = $Certificate; OwnedCertificates = $OwnedCertificates
            TrustCertificates = $TrustCertificates; PolicyToken = $PolicyToken
        }
    }

    $Now = [DateTime]::Now
    if ($Certificate.NotBefore -gt $Now) {
        return [pscustomobject]@{
            Name = 'NotYetValid'; Reason = 'The publisher certificate is not valid yet.'
            Certificate = $Certificate; OwnedCertificates = $OwnedCertificates
            TrustCertificates = $TrustCertificates; PolicyToken = $PolicyToken
        }
    }
    if ($Certificate.NotAfter -le $Now) {
        return [pscustomobject]@{
            Name = 'Expired'; Reason = 'The publisher certificate has expired.'
            Certificate = $Certificate; OwnedCertificates = $OwnedCertificates
            TrustCertificates = $TrustCertificates; PolicyToken = $PolicyToken
        }
    }

    $TrustCertificatePath = Join-Path `
        $Configuration.TrustCertificateStore `
        $Certificate.Thumbprint
    $TrustCertificate = Get-Item `
        -LiteralPath $TrustCertificatePath `
        -ErrorAction SilentlyContinue
    if ($null -eq $TrustCertificate) {
        return [pscustomobject]@{
            Name = 'RootTrustMissing'; Reason = 'The public publisher certificate is missing from CurrentUser\Root.'
            Certificate = $Certificate; OwnedCertificates = $OwnedCertificates
            TrustCertificates = $TrustCertificates; PolicyToken = $PolicyToken
        }
    }
    $TrustFingerprint = Get-RdpClientCertificateFingerprintSha256 `
        -Certificate $TrustCertificate
    if ($TrustFingerprint -ne $ActualFingerprint) {
        return [pscustomobject]@{
            Name = 'RootTrustMismatch'; Reason = 'The CurrentUser\Root certificate does not match the publisher identity.'
            Certificate = $Certificate; OwnedCertificates = $OwnedCertificates
            TrustCertificates = $TrustCertificates; PolicyToken = $PolicyToken
        }
    }

    $TrustedTokens = @(Get-RdpClientTrustedPublisherTokens -Configuration $Configuration)
    $IsTrusted = @($TrustedTokens | Where-Object {
        [string]::Equals($_, $PolicyToken, [StringComparison]::OrdinalIgnoreCase)
    }).Count -gt 0
    if (-not $IsTrusted) {
        return [pscustomobject]@{
            Name = 'TrustMissing'; Reason = 'The SHA-256 trusted RDP publisher policy entry is missing.'
            Certificate = $Certificate; OwnedCertificates = $OwnedCertificates
            TrustCertificates = $TrustCertificates; PolicyToken = $PolicyToken
        }
    }

    $RdpSignPath = Get-RdpClientRdpSignPath
    if (-not [IO.File]::Exists($RdpSignPath)) {
        return [pscustomobject]@{
            Name = 'ToolMissing'; Reason = "rdpsign.exe was not found: $RdpSignPath"
            Certificate = $Certificate; OwnedCertificates = $OwnedCertificates
            TrustCertificates = $TrustCertificates; PolicyToken = $PolicyToken
        }
    }

    return [pscustomobject]@{
        Name              = 'Ready'
        Reason            = ''
        Certificate       = $Certificate
        TrustCertificate  = $TrustCertificate
        OwnedCertificates = $OwnedCertificates
        TrustCertificates = $TrustCertificates
        PolicyToken       = $PolicyToken
        ExpiresSoon       = $Certificate.NotAfter -le $Now.AddDays(30)
    }
}

function Invoke-RdpClientRdpSignProcess {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)][string]$CertificateHash
    )

    $PreviousPreference = $ErrorActionPreference
    try {
        $ErrorActionPreference = 'Continue'
        $NativeOutput = & (Get-RdpClientRdpSignPath) `
            /sha256 $CertificateHash /q $Path 2>&1 | Out-String
        $ExitCode = $LASTEXITCODE
    } finally {
        $ErrorActionPreference = $PreviousPreference
    }

    return [pscustomobject]@{
        ExitCode = $ExitCode
        Output   = $NativeOutput.Trim()
    }
}

function Invoke-RdpClientRdpSignCompatible {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [Parameter(Mandatory = $true)]
        [Security.Cryptography.X509Certificates.X509Certificate2]$Certificate
    )

    # Microsoft documents /sha256 as accepting the certificate's SHA-256
    # fingerprint. Windows 11 build 26100 instead resolves CurrentUser\My by
    # the traditional SHA-1 Thumbprint even when /sha256 selects the signing
    # mode. Prefer the documented identifier, and retry only for the exact
    # certificate-not-found result so unrelated signing failures stay visible.
    $Sha256Fingerprint = Get-RdpClientCertificateFingerprintSha256 `
        -Certificate $Certificate
    $PrimaryResult = Invoke-RdpClientRdpSignProcess `
        -Path $Path `
        -CertificateHash $Sha256Fingerprint
    if ($PrimaryResult.ExitCode -eq 0) {
        return $PrimaryResult
    }
    if ($PrimaryResult.ExitCode -ne 0x80092004) {
        throw "rdpsign.exe failed with exit code $($PrimaryResult.ExitCode). $($PrimaryResult.Output)"
    }

    Write-Verbose (
        'rdpsign.exe could not resolve the SHA-256 certificate fingerprint; ' +
        'retrying with the certificate SHA-1 Thumbprint.'
    )
    $FallbackResult = Invoke-RdpClientRdpSignProcess `
        -Path $Path `
        -CertificateHash $Certificate.Thumbprint
    if ($FallbackResult.ExitCode -ne 0) {
        throw (
            'rdpsign.exe could not select the signing certificate by either ' +
            "supported identifier. SHA-256 exit code $($PrimaryResult.ExitCode): " +
            "$($PrimaryResult.Output) SHA-1 fallback exit code " +
            "$($FallbackResult.ExitCode): $($FallbackResult.Output)"
        )
    }
    return $FallbackResult
}

function Write-RdpClientSigningMissingNotice {
    param(
        [string]$CommandName = 'rdp',
        [switch]$Cached
    )

    if ($Cached) {
        Write-Host '[RDP] Signing:   not installed; cached file remains unsigned.'
    } else {
        Write-Host '[RDP] Signing:   not installed; file remains unsigned.'
    }
    Write-Host (
        '[RDP]            Run "{0} .sign install" to enable trusted signing.' -f `
            $CommandName
    )
}

function Invoke-RdpClientFileSigning {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [string]$CommandName = 'rdp',
        [pscustomobject]$Configuration = (Get-RdpClientSigningConfiguration)
    )

    $State = Get-RdpClientSigningState -Configuration $Configuration
    if ($State.Name -eq 'Missing') {
        Write-RdpClientSigningMissingNotice -CommandName $CommandName
        return $false
    }
    if ($State.Name -ne 'Ready') {
        throw "RDP signing state is $($State.Name): $($State.Reason) Run `"$CommandName .sign status`" for details."
    }

    $null = Invoke-RdpClientRdpSignCompatible `
        -Path $Path `
        -Certificate $State.Certificate

    $SignedText = [IO.File]::ReadAllText($Path, [Text.Encoding]::Unicode)
    if ($SignedText -notmatch '(?m)^signscope:s:' -or
        $SignedText -notmatch '(?m)^signature:s:') {
        throw 'rdpsign.exe returned success but the RDP signature is missing.'
    }
    Write-Host "[RDP] Signed:     $($Configuration.FriendlyName)"
    if ($State.ExpiresSoon) {
        Write-Warning "The RDP publisher certificate expires on $($State.Certificate.NotAfter.ToString('yyyy-MM-dd'))."
    }
    return $true
}
