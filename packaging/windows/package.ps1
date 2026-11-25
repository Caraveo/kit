# Package Kit for Windows as a .zip.
# usage (repo root, after `cargo build --release`): pwsh packaging/windows/package.ps1 [-Target x86_64-pc-windows-msvc]
param([string]$Target = "")
$ErrorActionPreference = "Stop"

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.*)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$binDir = if ($Target) { "target/$Target/release" } else { "target/release" }
$arch = if ($Target -like "aarch64*") { "arm64" } else { "x86_64" }
$name = "kit-$version-windows-$arch"
$stage = "dist/$name"
$ffmpegTarget = if ($Target) { $Target } else { (rustc -vV | Select-String '^host: (.*)').Matches[0].Groups[1].Value }
$ffmpegDir = "target/ffmpeg/$ffmpegTarget"
foreach ($file in 'ffmpeg.exe', 'COPYING.LGPLv2.1', 'LICENSE.md', 'build.sh', 'ffmpeg-8.1.3.tar.xz') {
    if (!(Test-Path "$ffmpegDir/$file" -PathType Leaf)) {
        throw "Build FFmpeg first in MSYS2 with packaging/ffmpeg/build.sh $ffmpegTarget"
    }
}


Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force "$stage/examples" | Out-Null
Copy-Item "$binDir/kit-editor.exe", "$binDir/kit-player.exe" $stage
Copy-Item -Recurse $ffmpegDir "$stage/ffmpeg"
New-Item -ItemType Directory -Force "$stage/players/web" | Out-Null
Copy-Item target/wasm32-unknown-unknown/release/kit_web.wasm "$stage/players/web/"
if (Test-Path dist/players) { Copy-Item -Recurse -Force dist/players/* "$stage/players/" }
Copy-Item -Recurse examples/hello "$stage/examples/"
Remove-Item -Recurse -Force "$stage/examples/hello/export" -ErrorAction SilentlyContinue
Copy-Item README.md $stage
Copy-Item LICENSE, BRANDING.md $stage
Compress-Archive -Path $stage -DestinationPath "dist/$name.zip" -Force
Remove-Item -Recurse -Force $stage
Write-Host "wrote dist/$name.zip"
