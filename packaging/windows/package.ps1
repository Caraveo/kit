# Package Kit for Windows as a portable ZIP and, with -Installer, a setup EXE.
param(
    [string]$Target = "",
    [switch]$Installer,
    [string]$InnoCompiler = "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe"
)
$ErrorActionPreference = "Stop"

function Get-VerifiedDownload($Url, $Destination, $Sha256) {
    Invoke-WebRequest $Url -OutFile $Destination
    if ((Get-FileHash -LiteralPath $Destination -Algorithm SHA256).Hash -ne $Sha256) {
        throw "Download checksum mismatch for $Url"
    }
}

$version = (Select-String -Path Cargo.toml -Pattern '^version = "(.*)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$binDir = if ($Target) { "target/$Target/release" } else { "target/release" }
$ffmpegTarget = if ($Target) { $Target } else { (rustc -vV | Select-String '^host: (.*)').Matches[0].Groups[1].Value }
$arch = if ($ffmpegTarget -like "aarch64*") { "arm64" } else { "x86_64" }
$name = "kit-$version-windows-$arch"
$stage = "dist/$name"
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
Write-Host "wrote dist/$name.zip"

if ($Installer) {
    $runtimeArch = if ($arch -eq "arm64") { "arm64" } else { "x64" }
    $architecture = if ($arch -eq "arm64") { "arm64" } else { "x64os" }
    $dependencies = Join-Path ([IO.Path]::GetTempPath()) "kit-setup-$([guid]::NewGuid())"
    New-Item -ItemType Directory $dependencies | Out-Null
    $redist = Join-Path $dependencies "vc_redist.$runtimeArch.exe"
    try {
        Invoke-WebRequest "https://aka.ms/vc14/vc_redist.$runtimeArch.exe" -OutFile $redist
        $signature = Get-AuthenticodeSignature $redist
        if ($signature.Status -ne "Valid" -or $signature.SignerCertificate.GetNameInfo("SimpleName", $false) -ne "Microsoft Corporation") {
            throw "The Visual C++ Redistributable signature could not be verified."
        }
        $compilerArgs = @(
            "/DKitVersion=$version", "/DKitArch=$arch", "/DKitArchitecture=$architecture"
            "/DKitRedist=$redist", "/DKitStage=$((Resolve-Path $stage).Path)", "/O$((Resolve-Path dist).Path)"
        )
        if ($arch -eq "x86_64") {
            Get-VerifiedDownload `
                "https://sdk.lunarg.com/sdk/download/1.4.363.0/windows/VulkanRT-X64-1.4.363.0-Installer.exe" `
                (Join-Path $dependencies "Kit-Vulkan-Runtime.exe") `
                "3f03bda2522d1d10d05461ce8a47364f9af6f5dc8642b7c45e43943dba164303"
            $driverArchive = Join-Path $dependencies "lavapipe.7z"
            Get-VerifiedDownload `
                "https://github.com/mmozeiko/build-mesa/releases/download/26.2.6/mesa-lavapipe-x64-26.2.6.7z" `
                $driverArchive "4ed980046a8d42de4efa74085f00e238a51c774b854d63ecece6dfee59ab3bf3"
            $sevenZip = Join-Path $env:ProgramFiles "7-Zip/7z.exe"
            if (!(Test-Path -LiteralPath $sevenZip)) { $sevenZip = "7z.exe" }
            & $sevenZip x $driverArchive "-o$(Join-Path $dependencies 'Driver')" -y
            if ($LASTEXITCODE -ne 0) { throw "Lavapipe extraction failed with exit code $LASTEXITCODE" }
            $compilerArgs += "/DKitVulkan=$dependencies"
        }
        & $InnoCompiler @compilerArgs "$PSScriptRoot/kit.iss"
        if ($LASTEXITCODE -ne 0) { throw "Inno Setup failed with exit code $LASTEXITCODE" }
        Write-Host "wrote dist/$name-setup.exe"
    } finally {
        Remove-Item -LiteralPath $dependencies -Recurse -Force
    }
}
Remove-Item -Recurse -Force $stage
