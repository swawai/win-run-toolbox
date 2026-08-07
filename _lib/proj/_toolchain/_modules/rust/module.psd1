@{
    Schema = 'swawkit.proj-dev.module.v0'
    Name = 'rust'
    ModeVariable = 'SWAWKIT_PROJ_RUST_MODE'
    SetupImplemented = $true
    ToolchainVariable = 'SWAWKIT_PROJ_RUST_TOOLCHAIN'
    ProfileVariable = 'SWAWKIT_PROJ_RUST_PROFILE'
    HostVariable = 'SWAWKIT_PROJ_RUST_HOST'
    InstallMode = 'rustup'
    RecipeVersion = '2'
    SupportedProfiles = @('minimal')
    SupportedHost = 'x86_64-pc-windows-msvc'
    RequiredComponents = @('rustfmt')
    RustupInit = @{
        Url = 'https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe'
        ChecksumUrl = 'https://static.rust-lang.org/rustup/dist/x86_64-pc-windows-msvc/rustup-init.exe.sha256'
    }
}
