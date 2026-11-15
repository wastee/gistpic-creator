# 在 Windows（MSVC 工具链）上打包成便携压缩包：dist\gistpic-creator-windows-x64.zip
$ErrorActionPreference = "Stop"
Set-Location (Join-Path $PSScriptRoot "..")

cargo build --release

$name = "gistpic-creator-windows-x64"
$dest = "dist\$name"

if (Test-Path $dest) { Remove-Item -Recurse -Force $dest }
New-Item -ItemType Directory -Force -Path $dest | Out-Null

Copy-Item "target\release\gistpic-creator.exe" $dest
Copy-Item "README.md" $dest
Copy-Item "THIRD_PARTY_NOTICES.md" $dest

$zip = "dist\$name.zip"
if (Test-Path $zip) { Remove-Item -Force $zip }
Compress-Archive -Path $dest -DestinationPath $zip

Write-Output "已生成 $zip"
