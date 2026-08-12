$ErrorActionPreference = "Stop"

$script:RepoRoot = Split-Path -Parent (Split-Path -Parent (Split-Path -Parent $PSScriptRoot))
$script:KitRoot = Join-Path $script:RepoRoot "_lib\ssh_remote_kit"

. (Join-Path $script:KitRoot "ps_common.ps1")
. (Join-Path $script:KitRoot "script_runner.openssh.ps1")

function Assert-True {
    param(
        [Parameter(Mandatory=$true)] [bool]$Condition,
        [Parameter(Mandatory=$true)] [string]$Message
    )

    if (-not $Condition) {
        throw $Message
    }
}

function Assert-Contains {
    param(
        [Parameter(Mandatory=$true)] [string]$Text,
        [Parameter(Mandatory=$true)] [string]$Expected,
        [Parameter(Mandatory=$true)] [string]$Message
    )

    Assert-True ($Text.Contains($Expected)) $Message
}

function Initialize-DummyContext {
    param(
        [string]$SshConfigPath = "",
        [string]$SshHostAlias = ""
    )

    $params = @{
        Port = 2222
        RemoteHost = "example.invalid"
        RemoteUser = "root"
        SshKeyPath = "C:\keys with spaces\id_rsa"
        ModuleRoot = $script:KitRoot
        UploadSubdir = "smoke_script_runner"
    }

    if (-not [string]::IsNullOrWhiteSpace($SshConfigPath)) {
        $params.SshConfigPath = $SshConfigPath
    }

    if (-not [string]::IsNullOrWhiteSpace($SshHostAlias)) {
        $params.SshHostAlias = $SshHostAlias
    }

    [void](Initialize-RemoteKitContext @params)
}

function Test-PayloadEmbedsScriptAndArgsForSingleSshConnection {
    $content = "#!/usr/bin/env bash`r`necho script-ok`r`n"
    $payload = New-RemoteKitScriptRunnerOpenSshPayload `
        -ScriptContent $content `
        -ScriptArgs @("alpha", "two words", "quote'value") `
        -Token "SMOKE_TOKEN"

    Assert-Contains $payload "cat > `"`$script_path`" <<'REMOTE_KIT_SMOKE_TOKEN_SCRIPT'" "payload should write script via heredoc."
    Assert-Contains $payload "echo script-ok" "payload should contain local script content."
    $expectedRun = @'
bash "$script_path" 'alpha' 'two words' 'quote'"'"'value'
'@.Trim()
    Assert-Contains $payload $expectedRun "payload should shell-quote forwarded script args."
    Assert-Contains $payload '${TMPDIR:-/tmp}/swaw-kit-ssh-remote.XXXXXXXXXX' "payload should use the canonical remote temp namespace."
    Assert-Contains $payload "trap cleanup EXIT" "payload should clean remote temp files."
    Assert-True (-not $payload.Contains("`r")) "payload should use LF line endings."
}

function Test-ScriptRunnerArgsAllowOneConnectionPasswordFallback {
    Initialize-DummyContext

    $args = @(New-RemoteKitScriptRunnerOpenSshArgs)
    $joined = $args -join " "

    Assert-Contains $joined "-o BatchMode=no" "script runner should allow one OpenSSH password prompt path."
    Assert-Contains $joined "-o PreferredAuthentications=publickey,password,keyboard-interactive" "script runner should try public key then password-compatible auth."
    Assert-Contains $joined "-p 2222" "script runner should include direct host port."
    Assert-Contains $joined "root@example.invalid" "script runner should include direct remote target."
    Assert-True ($args[-1] -eq "bash -s") "script runner should execute one remote bash stdin command."
    Assert-True (-not ($args -contains "-n")) "script runner must not close stdin."
}

function Test-ConfigHostArgsUseConfigAliasWithoutDirectOverrides {
    Initialize-DummyContext -SshConfigPath "D:\repo data\vps1.config" -SshHostAlias "vps1"

    $args = @(New-RemoteKitScriptRunnerOpenSshArgs)
    $joined = $args -join " "

    Assert-Contains $joined "-F D:\repo data\vps1.config" "config host mode should pass generated ssh config."
    Assert-Contains $joined "vps1 bash -s" "config host mode should use the Host alias as target."
    Assert-True (-not ($args -contains "-i")) "config host mode should not override IdentityFile with -i."
    Assert-True (-not ($args -contains "-p")) "config host mode should not override Port with -p."
    Assert-True (-not $joined.Contains("root@example.invalid")) "config host mode should not build user@host target."
}

function Test-PuttyFallbackCodeIsRemoved {
    $scriptRunner = [System.IO.File]::ReadAllText((Join-Path $script:KitRoot "script_runner.ps1"))
    $common = [System.IO.File]::ReadAllText((Join-Path $script:KitRoot "ps_common.ps1"))

    foreach ($needle in @("PuTTY", "plink", "pscp", "pwfile")) {
        Assert-True (-not $scriptRunner.Contains($needle)) "script_runner should not keep $needle fallback code."
        Assert-True (-not $common.Contains($needle)) "ps_common should not keep $needle helper code."
    }
}

function Test-WindowsProcessArgumentsPreserveEmptyAndEquals {
    $commandLine = Join-RemoteKitProcessArguments @(
        '-SshKeyPath',
        '',
        '-EncodedCommand',
        'ABC=='
    )

    Assert-True `
        ($commandLine -eq '-SshKeyPath "" -EncodedCommand ABC==') `
        'child PowerShell arguments should preserve empty values and trailing equals.'
}

function Test-GenericStdinRunnerContract {
    $kit = [IO.File]::ReadAllText((Join-Path $script:KitRoot 'kit.cmd'))
    $runtime = [IO.File]::ReadAllText((Join-Path $script:KitRoot 'kit.ps1'))
    $runner = [IO.File]::ReadAllText(
        (Join-Path $script:KitRoot 'stdin_runner.ps1')
    )
    $help = [IO.File]::ReadAllText(
        (Join-Path $script:KitRoot 'help\en.txt'),
        [Text.Encoding]::UTF8
    )

    Assert-Contains $kit '-File "%~dp0kit.ps1" %*' `
        'kit.cmd should forward the complete CMD argument text once.'
    Assert-Contains $kit 'chcp 65001 >nul <nul' `
        'kit.cmd must keep chcp from consuming redirected standard input.'
    Assert-True (-not ($kit -match '%~[5-9]')) `
        'kit.cmd must not parse data-bearing arguments through CMD numbered parameters.'
    Assert-True (-not ($kit -match '(?im)^\s*shift(?:\s|$)')) `
        'kit.cmd must not mutate the forwarded argument vector with SHIFT.'
    Assert-Contains $runtime "if (`$Verb -eq 'stdin')" `
        'kit.ps1 should dispatch the stdin verb.'
    Assert-Contains $runtime 'REMOTE_KIT_STDIN_ARG_COUNT' `
        'kit.ps1 should forward stdin remote arguments explicitly.'
    Assert-Contains $runtime '-RemoteShell $Runtime.RemoteShell' `
        'kit.ps1 should pass resolved shell metadata to the stdin runner.'
    Assert-Contains $runtime 'implemented for remote commands.' `
        'reserved remote-shell profiles should fail explicitly before SSH execution.'
    Assert-Contains $runner 'RedirectStandardInput = $false' `
        'stdin processes should inherit raw standard handles.'
    Assert-Contains $runner "`$_ -ne '-n'" `
        'posix stdin must remove the SSH option that closes stdin.'
    Assert-Contains $runner '$RemoteArguments -join '' ''' `
        'stdin runner should build one explicit remote command.'
    Assert-Contains $runner 'Invoke-RemoteKitWindowsCmdStdinCommand' `
        'Windows stdin should use its explicit staged transport.'
    Assert-Contains $runner 'Copy-RemoteKitStandardInputToPayload' `
        'Windows stdin should spool the raw local input before SSH execution.'
    Assert-Contains $runner "-ExePath 'scp.exe'" `
        'Windows stdin should upload the payload outside the SSH stdin channel.'
    Assert-Contains $runner 'CreateNoWindow = $false' `
        'Windows child commands must preserve inherited SSH output handles.'
    Assert-Contains $runner 'Remove-Item -LiteralPath $payloadPath' `
        'Windows stdin loader should remove its remote payload.'
    Assert-Contains $runner 'Remove-RemoteKitTempPath $LocalPayloadPath' `
        'Windows stdin should remove its local payload.'
    Assert-Contains $runner 'chcp 65001>nul <nul &' `
        'Windows stdin initialization must not consume the staged payload.'
    Assert-Contains $runner 'is recognized but not implemented for stdin commands' `
        'stdin runner should reject reserved profiles without an implementation.'
    Assert-Contains $help 'stdin -- command < file' `
        'SSH help should advertise the generic stdin command.'
    Assert-True (-not $runner.Contains('StandardInput.BaseStream')) `
        'stdin runner must not pump bytes into the OpenSSH stdin channel.'
}

function Test-GenericStdinKitDispatch {
    $scratch = Join-Path ([IO.Path]::GetTempPath()) (
        'swaw-kit-stdin-dispatch-' + [Guid]::NewGuid().ToString('N')
    )
    $capture = Join-Path $scratch 'capture.txt'
    $payload = Join-Path $scratch 'payload.txt'
    $previousCapture = $env:REMOTE_KIT_STDIN_TEST_CAPTURE
    $previousEntry = $env:REMOTE_KIT_ENTRY_FILE
    try {
        [IO.Directory]::CreateDirectory($scratch) | Out-Null
        [IO.File]::Copy(
            (Join-Path $script:KitRoot 'kit.cmd'),
            (Join-Path $scratch 'kit.cmd')
        )
        $payloadBytes = [byte[]]@(0, 1, 2, 10, 13, 26, 128, 255)
        [IO.File]::WriteAllBytes($payload, $payloadBytes)
        $fakeRuntime = @'
$inputStream = [Console]::OpenStandardInput()
$memory = New-Object IO.MemoryStream
$inputStream.CopyTo($memory)
$lines = @(
    "PayloadBase64=$([Convert]::ToBase64String($memory.ToArray()))",
    "Count=$($args.Count)"
)
for ($i = 0; $i -lt $args.Count; $i++) {
    $lines += "Arg$($i + 1)=$($args[$i])"
}
[IO.File]::WriteAllLines($env:REMOTE_KIT_STDIN_TEST_CAPTURE, $lines)
exit 0
'@
        [IO.File]::WriteAllText(
            (Join-Path $scratch 'kit.ps1'),
            $fakeRuntime,
            (New-Object Text.UTF8Encoding($false))
        )
        $env:REMOTE_KIT_STDIN_TEST_CAPTURE = $capture
        Remove-Item Env:REMOTE_KIT_ENTRY_FILE -ErrorAction SilentlyContinue
        $kitPath = Join-Path $scratch 'kit.cmd'
        $dispatch = (
            'call "' + $kitPath + '" 22 example.invalid root ' +
            'C:\keys\id_test stdin -- powershell.exe -NoLogo ' +
            '-EncodedCommand ABC== name=value < "' + $payload + '"'
        )
        & $env:ComSpec /d /c $dispatch
        Assert-True ($LASTEXITCODE -eq 0) 'kit.cmd argument bridge should succeed.'
        $state = @([IO.File]::ReadAllLines($capture))
        foreach ($expected in @(
            "PayloadBase64=$([Convert]::ToBase64String($payloadBytes))",
            'Count=11',
            'Arg1=22',
            'Arg2=example.invalid',
            'Arg3=root',
            'Arg4=C:\keys\id_test',
            'Arg5=stdin',
            'Arg6=--',
            'Arg7=powershell.exe',
            'Arg8=-NoLogo',
            'Arg9=-EncodedCommand',
            'Arg10=ABC==',
            'Arg11=name=value'
        )) {
            Assert-True ($state -contains $expected) `
                "CMD bridge is missing '$expected'."
        }

        $emptyDispatch = (
            'call "' + $kitPath + '" "0" "" "" "__ID__" ' +
            '-- echo ABC== < nul'
        )
        & $env:ComSpec /d /c $emptyDispatch
        Assert-True ($LASTEXITCODE -eq 0) `
            'kit.cmd bridge should preserve empty fixed arguments.'
        $emptyState = @([IO.File]::ReadAllLines($capture))
        foreach ($expected in @(
            'Count=7',
            'Arg1=0',
            'Arg2=',
            'Arg3=',
            'Arg4=__ID__',
            'Arg5=--',
            'Arg6=echo',
            'Arg7=ABC=='
        )) {
            Assert-True ($emptyState -contains $expected) `
                "CMD bridge empty-argument case is missing '$expected'."
        }
    } finally {
        [Environment]::SetEnvironmentVariable(
            'REMOTE_KIT_STDIN_TEST_CAPTURE',
            $previousCapture,
            'Process'
        )
        [Environment]::SetEnvironmentVariable(
            'REMOTE_KIT_ENTRY_FILE',
            $previousEntry,
            'Process'
        )
        if ([IO.Directory]::Exists($scratch)) {
            [IO.Directory]::Delete($scratch, $true)
        }
    }
}

try {
    Test-PayloadEmbedsScriptAndArgsForSingleSshConnection
    Test-ScriptRunnerArgsAllowOneConnectionPasswordFallback
    Test-ConfigHostArgsUseConfigAliasWithoutDirectOverrides
    Test-PuttyFallbackCodeIsRemoved
    Test-WindowsProcessArgumentsPreserveEmptyAndEquals
    Test-GenericStdinRunnerContract
    Test-GenericStdinKitDispatch
    Write-Host "ssh remote kit script-runner smoke ok" -ForegroundColor Green
} finally {
    $ctx = $null
    try { $ctx = Get-RemoteKitContext } catch { }
    if ($ctx -and (Test-Path -LiteralPath $ctx.UploadTempRoot)) {
        Remove-Item -LiteralPath $ctx.UploadTempRoot `
            -Recurse `
            -Force `
            -ErrorAction SilentlyContinue
    }
}
