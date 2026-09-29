$ErrorActionPreference = 'Stop'
Push-Location (Join-Path $PSScriptRoot 'threadtype-egui')
try {
    & cargo run --locked --bin pathweaver
    if ($LASTEXITCODE -ne 0) {
        throw "PathWeaver exited with code $LASTEXITCODE"
    }
} finally {
    Pop-Location
}
