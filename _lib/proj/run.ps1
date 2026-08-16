$CommandName = if ([string]::IsNullOrWhiteSpace($env:SWAWKIT_PROJ_ENTRY_COMMAND)) {
    'proj'
} else {
    $env:SWAWKIT_PROJ_ENTRY_COMMAND
}

Write-Host 'Swaw Kit Proj command tree is ready.'
Write-Host "Start the local web console: $CommandName"
Write-Host "Try: $CommandName .help, $CommandName .check <command-address>"
