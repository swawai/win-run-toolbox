$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

throw (
    'Proj logs are owned by the Rust Core and must be handled before command ' +
    'adapter execution.'
)
