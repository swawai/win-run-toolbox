$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2.0
[Console]::OutputEncoding = [Text.UTF8Encoding]::new($false)

throw (
    'Proj help is owned by the Rust Core and must be rendered before command ' +
    'adapter execution.'
)
