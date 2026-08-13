Set-StrictMode -Version 2.0

function Get-RdpClientDesktopScriptLiteral {
    param(
        [Parameter(Mandatory = $true)]
        [Management.Automation.Language.CommandElementAst]$Element,
        [Parameter(Mandatory = $true)][int]$LineNumber
    )

    if ($Element -is [Management.Automation.Language.StringConstantExpressionAst] -or
        $Element -is [Management.Automation.Language.ConstantExpressionAst]) {
        return $Element.Value
    }
    throw (
        "Desktop script line $LineNumber only accepts literal arguments; " +
        "found: $($Element.Extent.Text)"
    )
}

function Resolve-RdpClientDesktopScriptInteger {
    param(
        [AllowNull()]$Value,
        [Parameter(Mandatory = $true)][string]$Name,
        [Parameter(Mandatory = $true)][int]$LineNumber,
        [int]$Minimum = 0,
        [int]$Maximum = [int]::MaxValue
    )

    $Result = [int]0
    if (-not [int]::TryParse([string]$Value, [ref]$Result) -or
        $Result -lt $Minimum -or $Result -gt $Maximum) {
        throw (
            "Desktop script line $LineNumber requires $Name between " +
            "$Minimum and $Maximum."
        )
    }
    return $Result
}

function Resolve-RdpClientDesktopScriptOutputPath {
    param(
        [Parameter(Mandatory = $true)][string]$Value,
        [Parameter(Mandatory = $true)][string]$ScriptDirectory,
        [Parameter(Mandatory = $true)][int]$LineNumber
    )

    if ([string]::IsNullOrWhiteSpace($Value)) {
        throw "Desktop script line $LineNumber requires a screenshot path."
    }
    $Expanded = [Environment]::ExpandEnvironmentVariables($Value.Trim())
    if (-not [IO.Path]::IsPathRooted($Expanded)) {
        $Expanded = Join-Path $ScriptDirectory $Expanded
    }
    $Resolved = [IO.Path]::GetFullPath($Expanded)
    if (-not [string]::Equals(
        [IO.Path]::GetExtension($Resolved),
        '.png',
        [StringComparison]::OrdinalIgnoreCase
    )) {
        throw "Desktop script line $LineNumber must name a .png screenshot."
    }
    return $Resolved
}

function Read-RdpClientDesktopScript {
    param(
        [Parameter(Mandatory = $true)][string]$Path,
        [ValidateRange(1, 1048576)][int]$MaximumBytes = 65536,
        [ValidateRange(1, 256)][int]$MaximumSteps = 32,
        [ValidateRange(1, 64)][int]$MaximumScreenshots = 8
    )

    $Expanded = [Environment]::ExpandEnvironmentVariables($Path.Trim())
    $Resolved = [IO.Path]::GetFullPath($Expanded)
    if (-not [IO.File]::Exists($Resolved)) {
        throw "Desktop script was not found: $Resolved"
    }
    if (-not [string]::Equals(
        [IO.Path]::GetExtension($Resolved),
        '.ps1',
        [StringComparison]::OrdinalIgnoreCase
    )) {
        throw 'Desktop script must name a .ps1 file.'
    }
    $Length = (Get-Item -LiteralPath $Resolved).Length
    if ($Length -gt $MaximumBytes) {
        throw "Desktop script exceeds the $MaximumBytes-byte limit."
    }

    $Tokens = $null
    $Errors = $null
    $Ast = [Management.Automation.Language.Parser]::ParseFile(
        $Resolved,
        [ref]$Tokens,
        [ref]$Errors
    )
    if ($Errors.Count -gt 0) {
        throw (
            "Desktop script does not parse at line " +
            "$($Errors[0].Extent.StartLineNumber): $($Errors[0].Message)"
        )
    }
    if ($null -ne $Ast.ParamBlock -or $null -ne $Ast.DynamicParamBlock -or
        $null -ne $Ast.BeginBlock -or $null -ne $Ast.ProcessBlock -or
        $null -eq $Ast.EndBlock -or $Ast.UsingStatements.Count -ne 0 -or
        $null -ne $Ast.EndBlock.Traps) {
        throw 'Desktop script only accepts direct desktop action statements.'
    }

    $Statements = @($Ast.EndBlock.Statements)
    if ($Statements.Count -eq 0) {
        throw 'Desktop script must contain at least one desktop action.'
    }
    if ($Statements.Count -gt $MaximumSteps) {
        throw "Desktop script exceeds the $MaximumSteps-step limit."
    }

    $ScriptDirectory = [IO.Path]::GetDirectoryName($Resolved)
    $OutputPaths = [Collections.Generic.HashSet[string]]::new(
        [StringComparer]::OrdinalIgnoreCase
    )
    $Steps = New-Object 'Collections.Generic.List[object]'
    $ScreenshotCount = 0
    for ($Index = 0; $Index -lt $Statements.Count; $Index++) {
        $Statement = $Statements[$Index]
        $LineNumber = [int]$Statement.Extent.StartLineNumber
        if ($Statement -isnot [Management.Automation.Language.PipelineAst] -or
            $Statement.PipelineElements.Count -ne 1 -or
            $Statement.PipelineElements[0] -isnot
                [Management.Automation.Language.CommandAst]) {
            throw (
                "Desktop script line $LineNumber must contain one allowed " +
                'desktop action command.'
            )
        }
        $Command = $Statement.PipelineElements[0]
        if ($Command.Redirections.Count -ne 0 -or
            $Command.InvocationOperator -ne
                [Management.Automation.Language.TokenKind]::Unknown) {
            throw (
                "Desktop script line $LineNumber cannot use redirection or " +
                'an invocation operator.'
            )
        }
        $Name = [string]$Command.GetCommandName()
        $Arguments = @($Command.CommandElements | Select-Object -Skip 1)
        $Step = [ordered]@{
            Index      = $Index + 1
            Action     = ''
            LineNumber = $LineNumber
        }
        switch -Regex ($Name) {
            '^(?i:Screenshot)$' {
                if ($Arguments.Count -ne 1) {
                    throw "Desktop script line $LineNumber requires Screenshot <path>."
                }
                $OutputPath = Resolve-RdpClientDesktopScriptOutputPath `
                    -Value ([string](Get-RdpClientDesktopScriptLiteral `
                        -Element $Arguments[0] `
                        -LineNumber $LineNumber)) `
                    -ScriptDirectory $ScriptDirectory `
                    -LineNumber $LineNumber
                if (-not $OutputPaths.Add($OutputPath)) {
                    throw "Desktop script repeats screenshot output: $OutputPath"
                }
                if ([IO.File]::Exists($OutputPath)) {
                    throw "Screenshot output already exists: $OutputPath"
                }
                $ScreenshotCount++
                if ($ScreenshotCount -gt $MaximumScreenshots) {
                    throw (
                        'Desktop script exceeds the ' +
                        "$MaximumScreenshots-screenshot limit."
                    )
                }
                $Step.Action = 'screenshot'
                $Step.OutputPath = $OutputPath
                break
            }
            '^(?i:Pixel|Click)$' {
                if ($Arguments.Count -ne 2) {
                    throw "Desktop script line $LineNumber requires $Name <x> <y>."
                }
                $Step.Action = $Name.ToLowerInvariant()
                $Step.X = Resolve-RdpClientDesktopScriptInteger `
                    -Value (Get-RdpClientDesktopScriptLiteral `
                        -Element $Arguments[0] `
                        -LineNumber $LineNumber) `
                    -Name X `
                    -LineNumber $LineNumber
                $Step.Y = Resolve-RdpClientDesktopScriptInteger `
                    -Value (Get-RdpClientDesktopScriptLiteral `
                        -Element $Arguments[1] `
                        -LineNumber $LineNumber) `
                    -Name Y `
                    -LineNumber $LineNumber
                break
            }
            '^(?i:Wait-Desktop)$' {
                if ($Arguments.Count -ne 1) {
                    throw (
                        "Desktop script line $LineNumber requires " +
                        'Wait-Desktop <milliseconds>.'
                    )
                }
                $Step.Action = 'wait'
                $Step.Milliseconds = Resolve-RdpClientDesktopScriptInteger `
                    -Value (Get-RdpClientDesktopScriptLiteral `
                        -Element $Arguments[0] `
                        -LineNumber $LineNumber) `
                    -Name milliseconds `
                    -LineNumber $LineNumber `
                    -Maximum 10000
                break
            }
            default {
                throw (
                    "Desktop script line $LineNumber uses unsupported action " +
                    "'$Name'. Allowed: Screenshot, Pixel, Click, Wait-Desktop."
                )
            }
        }
        $Steps.Add([pscustomobject]$Step)
    }

    return [pscustomobject]@{
        Path  = $Resolved
        Steps = $Steps.ToArray()
    }
}
