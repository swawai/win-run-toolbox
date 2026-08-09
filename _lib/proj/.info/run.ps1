if ($args.Count -ne 0) {
    throw "Command '.info' does not accept tail arguments."
}

$Rows = [ordered]@{
    command = $env:SWAWKIT_PROJ_CORE_COMMAND_ADDRESS
    commandDirectory = $env:SWAWKIT_PROJ_CORE_COMMAND_DIR
    entryName = $env:SWAWKIT_PROJ_ENTRY_COMMAND
    entryFile = $env:SWAWKIT_PROJ_CORE_COMMAND_ENTRY_FILE
    swawkitHome = $env:SWAWKIT_HOME
    targetProjectRoot = $env:SWAWKIT_PROJ_TARGET_PROJECT_ROOT
    actionRoot = $env:SWAWKIT_PROJ_ACTION_ROOT
    dataRoot = $env:SWAWKIT_PROJ_DATA_ROOT
    cacheRoot = Join-Path $env:SWAWKIT_HOME 'data\proj_cache'
    invocationDirectory = $env:SWAWKIT_PROJ_CORE_COMMAND_INVOCATION_DIR
}

foreach ($Name in $Rows.Keys) {
    $Value = if ($null -eq $Rows[$Name]) { '' } else { [string]$Rows[$Name] }
    Write-Host ("{0,-20} {1}" -f $Name, $Value)
}
