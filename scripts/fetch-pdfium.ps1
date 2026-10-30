# Downloads pdfium.dll (the build matching pdfium-render's API) into vendor\pdfium\win-x64.
$ErrorActionPreference = "Stop"
$build = if ($env:PDFIUM_BUILD) { $env:PDFIUM_BUILD } else { "7881" }
$root = Split-Path -Parent $PSScriptRoot
$dest = Join-Path $root "vendor\pdfium\win-x64"
New-Item -ItemType Directory -Force -Path $dest | Out-Null
$tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("pdfium-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $tmp | Out-Null
$url = "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F$build/pdfium-win-x64.tgz"
Invoke-WebRequest -Uri $url -OutFile (Join-Path $tmp "pdfium.tgz")
tar -xzf (Join-Path $tmp "pdfium.tgz") -C $tmp
Copy-Item (Join-Path $tmp "bin\pdfium.dll") $dest -Force
Copy-Item (Join-Path $tmp "LICENSE") (Join-Path $root "vendor\pdfium\PDFIUM-LICENSE") -Force
Remove-Item -Recurse -Force $tmp
Write-Host "pdfium $build -> $dest"
