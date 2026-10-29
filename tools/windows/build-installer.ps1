# Build the installer on Windows with Qt's MinGW kit and deploy its Qt
# libraries beside it (M21, docs/decisions/0005-installer.md).
#
#   pwsh -File build-installer.ps1 -Source DIR -Build DIR -Out DIR -Notices DIR [-Qt DIR] [-Tools DIR]
#
# Source is the repository's installer/ folder; Out receives
# GauntletDarkLegacy-Installer.exe with the Qt DLLs, plugins and MinGW
# runtime that windeployqt selects, and deployed-files.txt (each file's
# SHA-256, size and path). Notices receives what their license notices are
# made from (tools/python/library_licenses.py): Qt's SBOM of qtbase and the
# kit's MinGW license files. tools/package.sh windows runs this over SSH
# (GDL_WINDOWS_HOST) and packages Out. Pinned kit: docs/toolchain.md.
param(
    [Parameter(Mandatory)] [string] $Source,
    [Parameter(Mandatory)] [string] $Build,
    [Parameter(Mandatory)] [string] $Out,
    [Parameter(Mandatory)] [string] $Notices,
    [string] $Qt = 'C:\Qt\6.11.1\mingw_64',
    [string] $Tools = 'C:\Qt\Tools'
)
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true

$env:PATH = "$Tools\mingw1310_64\bin;$Tools\Ninja;$Tools\CMake_64\bin;$Qt\bin;" + $env:PATH
"qt: $Qt"
"g++: " + (& g++ --version | Select-Object -First 1)
"cmake: " + (& cmake --version | Select-Object -First 1)
"ninja: " + (& ninja --version)

cmake -S $Source -B $Build -G Ninja -DCMAKE_BUILD_TYPE=Release "-DCMAKE_PREFIX_PATH=$Qt" `
    "-DCMAKE_CXX_COMPILER=$Tools\mingw1310_64\bin\g++.exe" | Out-Null
cmake --build $Build

if (Test-Path $Out) { Remove-Item -Recurse -Force $Out }
New-Item -ItemType Directory -Force $Out | Out-Null
Copy-Item "$Build\GauntletDarkLegacy-Installer.exe" $Out
# A Widgets wizard without images or network use: Core, Gui, Widgets, the
# Windows platform and style plugins and the MinGW runtime. No
# translations (its own text is English), shader compilers, software
# OpenGL, Network/Svg or image, icon, TLS and touch plugins.
windeployqt --release --no-translations --no-system-d3d-compiler --no-system-dxc-compiler `
    --no-opengl-sw --compiler-runtime --no-network --no-svg `
    --skip-plugin-types generic,iconengines,imageformats,networkinformation,tls `
    "$Out\GauntletDarkLegacy-Installer.exe" | Out-Null

Push-Location $Out
try {
    $lines = Get-ChildItem -Recurse -File | Where-Object Name -ne 'deployed-files.txt' | Sort-Object FullName | ForEach-Object {
        $rel = (Resolve-Path -Relative $_.FullName).Substring(2).Replace('\', '/')
        '{0}  {1,9}  {2}' -f (Get-FileHash -Algorithm SHA256 $_.FullName).Hash.ToLower(), $_.Length, $rel
    }
    # LF line ends: tools/package.sh reads it on Linux.
    [IO.File]::WriteAllText("$Out\deployed-files.txt", ($lines -join "`n") + "`n")
} finally { Pop-Location }
Get-Content "$Out\deployed-files.txt"

if (Test-Path $Notices) { Remove-Item -Recurse -Force $Notices }
New-Item -ItemType Directory -Force "$Notices\mingw" | Out-Null
Copy-Item (Get-ChildItem "$Qt\sbom\qtbase-*.spdx.json").FullName $Notices
foreach ($d in 'gcc', 'mingw-w64', 'winpthreads') { Copy-Item -Recurse "$Tools\mingw1310_64\licenses\$d" "$Notices\mingw\" }
