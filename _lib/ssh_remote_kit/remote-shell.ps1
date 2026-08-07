function Get-RemoteKitRemoteShell {
    param([Parameter(Mandatory=$true)] [string[]]$Lines)

    $declarations = New-Object System.Collections.Generic.List[string]
    $directivePattern = '^\s*___RemoteShell___(?:\s|$)'
    $valuePattern = '^\s*___RemoteShell___\s+(?<Value>[A-Za-z0-9.-]+)\s*(?:#.*)?$'
    foreach ($line in $Lines) {
        if ($line -notmatch $directivePattern) {
            continue
        }
        if ($line -notmatch $valuePattern) {
            throw 'Malformed ___RemoteShell___ directive. Expected: ___RemoteShell___ <profile>'
        }
        $declarations.Add($Matches['Value'].ToLowerInvariant())
    }

    if ($declarations.Count -eq 0) {
        return 'posix'
    }
    if ($declarations.Count -ne 1) {
        throw 'Embedded ssh_config must contain at most one active ___RemoteShell___ directive.'
    }

    $shell = $declarations[0]
    $knownShells = @(
        'posix',
        'win.cmd',
        'win.powershell',
        'win.pwsh',
        'win.git-bash'
    )
    if ($shell -notin $knownShells) {
        throw "Unknown remote shell profile '$shell'."
    }
    return $shell
}

function Test-RemoteKitRemoteShellDirectiveLine {
    param([Parameter(Mandatory=$true)] [string]$Line)

    return (
        $Line -match '^\s*___RemoteShell___(?:\s|$)' -or
        $Line -match '^\s*#\s*___RemoteShell___(?:\s|$)'
    )
}
