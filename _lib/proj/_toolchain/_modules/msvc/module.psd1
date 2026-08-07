@{
    Schema = 'swawkit.proj-dev.module.v0'
    Name = 'msvc'
    ModeVariable = 'SWAWKIT_PROJ_MSVC_MODE'
    SetupImplemented = $true
    ChannelVariable = 'SWAWKIT_PROJ_MSVC_CHANNEL'
    InstallMode = 'managed'
    RecipeVersion = '1'
    ChannelUrlTemplate = 'https://aka.ms/vs/{channel}/release/channel'
    VisualStudioManifestId = 'Microsoft.VisualStudio.Manifests.VisualStudio'
    ResourceLanguage = 'en-US'
    ToolPackageTemplates = @(
        'microsoft.vc.{tool}.crt.headers.base'
        'microsoft.vc.{tool}.crt.source.base'
        'microsoft.vc.{tool}.tools.hostx64.targetx64.base'
        'microsoft.vc.{tool}.tools.hostx64.targetx64.res.base'
        'microsoft.vc.{tool}.crt.x64.desktop.base'
        'microsoft.vc.{tool}.crt.x64.store.base'
        'microsoft.visualcpp.dia.sdk'
    )
    SdkMsiNames = @(
        'Windows SDK for Windows Store Apps Tools-x86_en-us.msi'
        'Windows SDK for Windows Store Apps Headers-x86_en-us.msi'
        'Windows SDK for Windows Store Apps Headers OnecoreUap-x86_en-us.msi'
        'Windows SDK for Windows Store Apps Libs-x86_en-us.msi'
        'Universal CRT Headers Libraries and Sources-x86_en-us.msi'
        'Windows SDK Desktop Headers x64-x86_en-us.msi'
        'Windows SDK OnecoreUap Headers x64-x86_en-us.msi'
        'Windows SDK Desktop Libs x64-x86_en-us.msi'
    )
}
