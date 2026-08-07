# Stable PowerShell release discovery and installation contract.
@{
    Schema = 'swawkit.proj-dev.module.v0'
    Name = 'pwsh'
    ModeVariable = 'SWAWKIT_PROJ_PWSH_MODE'
    SetupImplemented = $true
    VersionVariable = 'SWAWKIT_PROJ_PWSH_VERSION'
    HashVariable = 'SWAWKIT_PROJ_PWSH_SHA256'
    InstallMode = 'managed'
    RecipeVersion = 'pwsh-win-x64-zip-v0'
    Executable = 'pwsh.exe'
    RequiredPaths = @(
        'pwsh.exe'
    )
    Release = @{
        Provider = 'github'
        Repository = 'PowerShell/PowerShell'
        ApiVersion = '2026-03-10'
        TagTemplate = 'v{version}'
        AssetTemplate = 'PowerShell-{version}-win-x64.zip'
    }
}
