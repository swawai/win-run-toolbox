Set-StrictMode -Version 2.0

function Resolve-RdpClientHostAlias {
    param([AllowNull()][AllowEmptyString()][string]$Value)

    if ([string]::IsNullOrWhiteSpace($Value)) {
        return ''
    }

    $Alias = $Value.Trim()
    if ([Uri]::CheckHostName($Alias) -ne [UriHostNameType]::Dns) {
        throw 'RDP_HOST_ALIAS must be a valid DNS host name without a port.'
    }
    return $Alias
}

function Resolve-RdpClientSessionId {
    param(
        [AllowNull()][AllowEmptyString()][string]$Value,
        [string]$Label = 'Session ID'
    )

    if ([string]::IsNullOrWhiteSpace($Value)) {
        return $null
    }
    if ($Value -notmatch '^[0-9]+$') {
        throw "$Label must contain decimal digits only."
    }

    $SessionId = [uint32]0
    if (-not [uint32]::TryParse($Value, [ref]$SessionId)) {
        throw "$Label is outside the supported range: $Value"
    }
    return $SessionId
}

function Resolve-RdpClientShadowSessionId {
    param([AllowNull()][AllowEmptyString()][string]$Value)

    return Resolve-RdpClientSessionId `
        -Value $Value `
        -Label 'Shadow session ID'
}

function New-RdpClientShadowMstscArgumentList {
    param(
        [Parameter(Mandatory = $true)][string]$Target,
        [Parameter(Mandatory = $true)][uint32]$ShadowSessionId,
        [switch]$Control,
        [switch]$NoConsentPrompt
    )

    $ShadowTarget = Resolve-RdpClientShadowConnectionTarget -Target $Target
    $Arguments = New-Object 'Collections.Generic.List[string]'
    $Arguments.Add(('/v:{0}' -f $ShadowTarget))
    $Arguments.Add(('/shadow:{0}' -f $ShadowSessionId))
    if ($Control) {
        $Arguments.Add('/control')
    }
    if ($NoConsentPrompt) {
        $Arguments.Add('/noConsentPrompt')
    }
    return $Arguments.ToArray()
}

function Resolve-RdpClientShadowConnectionTarget {
    param([Parameter(Mandatory = $true)][string]$Target)

    $Address = Split-RdpClientFullAddress -Address $Target
    if ($null -ne $Address.Port -and [int]$Address.Port -ne 3389) {
        return $Address.Value
    }

    $ParsedIp = $null
    if ([Net.IPAddress]::TryParse($Address.Host, [ref]$ParsedIp) -and
        $ParsedIp.AddressFamily -eq [Net.Sockets.AddressFamily]::InterNetworkV6) {
        return "[$($Address.Host)]"
    }
    return $Address.Host
}

function Split-RdpClientFullAddress {
    param([Parameter(Mandatory = $true)][string]$Address)

    $Value = $Address.Trim()
    if ($Value.Length -eq 0) {
        throw 'The full address RDP property cannot be empty.'
    }

    $HostName = $null
    $Port = $null
    $ParsedIp = $null
    if ([Net.IPAddress]::TryParse($Value, [ref]$ParsedIp)) {
        $HostName = $Value
    } elseif ($Value -match '^\[(?<Host>[^\]]+)\](?::(?<Port>[0-9]+))?$') {
        $HostName = $Matches.Host
        if ($Matches.Port.Length -gt 0) {
            $Port = [int]$Matches.Port
        }
    } elseif ($Value -match '^(?<Host>[^:\s]+)(?::(?<Port>[0-9]+))?$') {
        $HostName = $Matches.Host
        if ($Matches.Port.Length -gt 0) {
            $Port = [int]$Matches.Port
        }
    } else {
        throw "Unsupported full address value: $Value"
    }

    if ($null -ne $Port -and ($Port -lt 1 -or $Port -gt 65535)) {
        throw "The RDP port must be between 1 and 65535: $Port"
    }

    return [pscustomobject]@{
        Value = $Value
        Host  = $HostName
        Port  = $Port
    }
}

function Get-RdpClientEmbeddedLines {
    param([Parameter(Mandatory = $true)][string]$Path)

    if (-not [IO.File]::Exists($Path)) {
        throw "RDP entry file not found: $Path"
    }

    $Lines = [IO.File]::ReadAllLines($Path, [Text.Encoding]::UTF8)
    $StartPattern = '^\s*goto\s+:RdpClientAfterEmbeddedRdpProperties\s*$'
    $EndPattern = '^\s*:RdpClientAfterEmbeddedRdpProperties\s*$'
    $StartIndexes = @()
    $EndIndexes = @()

    for ($Index = 0; $Index -lt $Lines.Length; $Index++) {
        if ($Lines[$Index] -match $StartPattern) {
            $StartIndexes += $Index
        }
        if ($Lines[$Index] -match $EndPattern) {
            $EndIndexes += $Index
        }
    }

    if ($StartIndexes.Count -ne 1 -or $EndIndexes.Count -ne 1) {
        throw 'The entry must contain exactly one embedded RDP goto and one matching label.'
    }

    $Start = $StartIndexes[0]
    $End = $EndIndexes[0]
    if ($End -le ($Start + 1)) {
        throw 'The embedded RDP block cannot be empty.'
    }
    return @($Lines[($Start + 1)..($End - 1)])
}

function Read-RdpClientEntryDocument {
    param([Parameter(Mandatory = $true)][string]$Path)

    $SourceLines = Get-RdpClientEmbeddedLines -Path $Path
    $Properties = New-Object 'Collections.Generic.List[object]'
    $Seen = @{}
    $FullAddress = $null
    $Username = $null

    for ($Index = 0; $Index -lt $SourceLines.Count; $Index++) {
        $Trimmed = $SourceLines[$Index].Trim()
        if ($Trimmed.Length -eq 0 -or $Trimmed.StartsWith('::')) {
            continue
        }
        if ($Trimmed -notmatch '^(?<Name>[^:]+):(?<Type>[sib]):(?<Value>.*)$') {
            throw "Invalid embedded RDP property at block line $($Index + 1): $Trimmed"
        }

        $Name = $Matches.Name.Trim()
        $Type = $Matches.Type.ToLowerInvariant()
        $Value = $Matches.Value
        $Key = $Name.ToLowerInvariant()
        if ($Seen.ContainsKey($Key)) {
            throw "Duplicate embedded RDP property: $Name"
        }
        $Seen[$Key] = $true

        if ($Key -eq 'full address') {
            if ($Type -ne 's') {
                throw 'The full address RDP property must use string type s.'
            }
            $FullAddress = Split-RdpClientFullAddress -Address $Value
        }
        if ($Key -eq 'username') {
            if ($Type -ne 's') {
                throw 'The username RDP property must use string type s.'
            }
            $Username = $Value.Trim()
            if ($Username.Length -eq 0) {
                throw 'The username RDP property cannot be empty.'
            }
        }

        $Properties.Add([pscustomobject]@{
            Name  = $Name
            Key   = $Key
            Type  = $Type
            Value = $Value
        })
    }

    if ($null -eq $FullAddress) {
        throw 'The embedded RDP block must contain full address:s:.'
    }
    if ($null -eq $Username) {
        throw 'The embedded RDP block must contain username:s:.'
    }

    return [pscustomobject]@{
        Properties  = $Properties.ToArray()
        FullAddress = $FullAddress
        Username    = $Username
    }
}

function ConvertTo-RdpClientOutputLines {
    param(
        [Parameter(Mandatory = $true)][pscustomobject]$Document,
        [AllowEmptyString()][string]$HostAlias
    )

    $Lines = New-Object 'Collections.Generic.List[string]'
    foreach ($Property in $Document.Properties) {
        if (@('signature', 'signscope') -contains $Property.Key) {
            continue
        }

        $Value = $Property.Value
        if ($Property.Key -eq 'full address') {
            $Value = Resolve-RdpClientConnectionTarget `
                -Document $Document `
                -HostAlias $HostAlias
        }
        $Lines.Add(('{0}:{1}:{2}' -f $Property.Name, $Property.Type, $Value))
    }
    return $Lines.ToArray()
}

function Resolve-RdpClientConnectionTarget {
    param(
        [Parameter(Mandatory = $true)][pscustomobject]$Document,
        [AllowEmptyString()][string]$HostAlias
    )

    if ($HostAlias.Length -eq 0) {
        return $Document.FullAddress.Value
    }

    $RealHostAddress = $null
    if (-not [Net.IPAddress]::TryParse(
        $Document.FullAddress.Host,
        [ref]$RealHostAddress
    )) {
        throw 'RDP_HOST_ALIAS requires full address to use an IPv4 or IPv6 address; a DNS source name cannot be mapped through the hosts file.'
    }

    $Target = $HostAlias
    if ($null -ne $Document.FullAddress.Port) {
        $Target += ':' + $Document.FullAddress.Port
    }
    return $Target
}

function Assert-RdpClientHostAliasResolves {
    param(
        [AllowEmptyString()][string]$HostAlias,
        [string]$CommandName = 'rdp'
    )

    if ($HostAlias.Length -eq 0) {
        return
    }

    try {
        $ResolvedAddresses = @([Net.Dns]::GetHostAddresses($HostAlias))
    } catch {
        throw (
            "RDP_HOST_ALIAS does not resolve: $HostAlias.`n" +
            "        Run `"$CommandName .hosts install --uac`"."
        )
    }
    if ($ResolvedAddresses.Count -eq 0) {
        throw (
            "RDP_HOST_ALIAS does not resolve: $HostAlias.`n" +
            "        Run `"$CommandName .hosts install --uac`"."
        )
    }
}
