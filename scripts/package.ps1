param(
    [switch] $SkipVerify
)

$ErrorActionPreference = "Stop"
$projectRoot = Split-Path $PSScriptRoot -Parent
Set-Location $projectRoot

$metadata = & cargo metadata --format-version 1 --no-deps | ConvertFrom-Json
$manifestPath = (Resolve-Path "Cargo.toml").Path
$package = $metadata.packages |
    Where-Object { [IO.Path]::GetFullPath($_.manifest_path) -eq $manifestPath } |
    Select-Object -First 1

if (-not $package) {
    throw "Could not read the root package from cargo metadata."
}

$version = $package.version
$packagerVersion = Select-String -Path "Packager.toml" -Pattern '^version\s*=\s*"([^\"]+)"$'
if (-not $packagerVersion -or $packagerVersion.Matches[0].Groups[1].Value -ne $version) {
    throw "Cargo.toml and Packager.toml versions must both be $version."
}

$packagerCommand = Get-Command packager -ErrorAction SilentlyContinue
if (-not $packagerCommand) {
    throw "packager was not found. Install it with: npm install --global @crabnebula/packager@0.11.2"
}

if (-not $SkipVerify) {
    & $PSScriptRoot\cargo.ps1 fmt --all --check
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    & $PSScriptRoot\cargo.ps1 clippy-strict --all-targets --all-features
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

    & $PSScriptRoot\cargo.ps1 test --all-targets --all-features
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
}

& $PSScriptRoot\cargo.ps1 build --release --locked
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

if (Test-Path "dist") {
    Remove-Item -LiteralPath "dist" -Recurse -Force
}
if (Test-Path ".cargo-packager") {
    Remove-Item -LiteralPath ".cargo-packager" -Recurse -Force
}

$packagerModule = Join-Path (Split-Path $packagerCommand.Source -Parent) "node_modules\@crabnebula\packager\index.js"
if (-not (Test-Path -LiteralPath $packagerModule)) {
    throw "Could not locate the native @crabnebula/packager module at $packagerModule."
}

# The npm wrapper auto-detects an Electron package.json in parent directories. Invoke the native
# binding directly so this standalone Rust project uses only its explicit Packager.toml.
$packagerScript = "require(process.argv[1]).cli(['--config', process.argv[2]], 'packager')"
& node -e $packagerScript $packagerModule (Resolve-Path "Packager.toml").Path
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

$installers = @(Get-ChildItem "dist" -File -Filter "*.exe")
if ($installers.Count -ne 1) {
    throw "Expected one NSIS installer in dist, found $($installers.Count)."
}

$releaseName = "SSH Tunnel Panel Setup $version.exe"
$releasePath = Join-Path "dist" $releaseName
Move-Item -LiteralPath $installers[0].FullName -Destination $releasePath -Force

$hash = (Get-FileHash -LiteralPath $releasePath -Algorithm SHA256).Hash.ToLowerInvariant()
Set-Content -LiteralPath "$releasePath.sha256" -Value "$hash  $releaseName" -Encoding ascii

Get-Item -LiteralPath $releasePath, "$releasePath.sha256"
