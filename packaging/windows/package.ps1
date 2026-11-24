# Package Kit for Windows as a .zip.
# usage (repo root, after `cargo build --release`): pwsh packaging/windows/package.ps1 [-Target x86_64-pc-windows-msvc]
param([string]$Target = "")
$ErrorActionPreference = "Stop"

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.*)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$binDir = if ($Target) { "target/$Target/release" } else { "target/release" }
$arch = if ($Target -like "aarch64*") { "arm64" } else { "x86_64" }
$name = "kit-$version-windows-$arch"
$stage = "dist/$name"

Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force "$stage/examples" | Out-Null
Copy-Item "$binDir/kit-editor.exe", "$binDir/kit-player.exe" $stage
New-Item -ItemType Directory -Force "$stage/players/web" | Out-Null
Copy-Item target/wasm32-unknown-unknown/release/kit_web.wasm "$stage/players/web/"
Copy-Item -Recurse examples/hello "$stage/examples/"
Copy-Item README.md $stage
if (Test-Path LICENSE) { Copy-Item LICENSE $stage }
Compress-Archive -Path $stage -DestinationPath "dist/$name.zip" -Force
Remove-Item -Recurse -Force $stage
Write-Host "wrote dist/$name.zip"
