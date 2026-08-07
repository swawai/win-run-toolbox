# Stable Bun release discovery and installation contract.
@{
    Schema = 'swawkit.proj-dev.module.v0'
    Name = 'bun'
    ModeVariable = 'SWAWKIT_PROJ_BUN_MODE'
    SetupImplemented = $true
    VersionVariable = 'SWAWKIT_PROJ_BUN_VERSION'
    HashVariable = 'SWAWKIT_PROJ_BUN_SHA256'
    InstallMode = 'managed'
    # Increment when a generated shim or installation behavior changes.
    RecipeVersion = '2'
    Executable = 'bun.exe'
    RequiredPaths = @(
        'bun.exe'
        'bunx.cmd'
    )
    Release = @{
        Provider = 'github'
        Repository = 'oven-sh/bun'
        ApiVersion = '2026-03-10'
        TagTemplate = 'bun-v{version}'
        Asset = 'bun-windows-x64.zip'
        ArchiveSubdir = 'bun-windows-x64'
    }
}
