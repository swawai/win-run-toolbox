Set-StrictMode -Version 2.0

function ConvertTo-ProjArgumentPayload {
    param(
        [Parameter(Mandatory = $true)]
        [AllowNull()]
        [AllowEmptyCollection()]
        [AllowEmptyString()]
        [string[]]$Arguments
    )

    $Values = [Collections.Generic.List[string]]::new()
    foreach ($Argument in @($Arguments)) {
        [void]$Values.Add([string]$Argument)
    }
    $Parts = [Collections.Generic.List[string]]::new()
    [void]$Parts.Add('proj.args.v1')
    [void]$Parts.Add($Values.Count.ToString(
        [Globalization.CultureInfo]::InvariantCulture
    ))
    foreach ($Argument in $Values.ToArray()) {
        [void]$Parts.Add([Convert]::ToBase64String(
            [Text.Encoding]::UTF8.GetBytes([string]$Argument)
        ))
    }
    return [string]::Join(';', $Parts.ToArray())
}

function ConvertFrom-ProjArgumentPayload {
    param([Parameter(Mandatory = $true)][string]$Payload)

    $Parts = [string[]]$Payload.Split(
        [char[]]@(';'),
        [StringSplitOptions]::None
    )
    [int]$ArgumentCount = 0
    if ($Parts.Count -lt 2 -or
        $Parts[0] -cne 'proj.args.v1' -or
        -not [int]::TryParse($Parts[1], [ref]$ArgumentCount) -or
        $ArgumentCount -lt 0 -or
        $Parts.Count -ne $ArgumentCount + 2) {
        throw 'The internal test argument payload is invalid.'
    }

    $Arguments = [Collections.Generic.List[string]]::new()
    for ($Index = 0; $Index -lt $ArgumentCount; $Index++) {
        try {
            $Value = [Text.Encoding]::UTF8.GetString(
                [Convert]::FromBase64String($Parts[$Index + 2])
            )
        } catch {
            throw 'The internal test argument payload is invalid.'
        }
        [void]$Arguments.Add($Value)
    }
    return $Arguments.ToArray()
}
