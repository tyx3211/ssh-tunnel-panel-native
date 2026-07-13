param(
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]] $CargoArguments
)

$ErrorActionPreference = "Stop"

if (-not $env:GPUI_FXC_PATH) {
    $sdkBin = Join-Path ${env:ProgramFiles(x86)} "Windows Kits\10\bin"
    $fxc = Get-ChildItem $sdkBin -Recurse -Filter fxc.exe -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match "\\x64\\fxc\.exe$" } |
        Sort-Object FullName -Descending |
        Select-Object -First 1

    if (-not $fxc) {
        throw "fxc.exe was not found. Install the Windows 10/11 SDK or set GPUI_FXC_PATH."
    }

    $env:GPUI_FXC_PATH = $fxc.FullName
}

if ($CargoArguments.Count -gt 0 -and $CargoArguments[0] -eq "clippy-strict") {
    $remaining = $CargoArguments | Select-Object -Skip 1
    & cargo clippy @remaining -- -D warnings
} else {
    & cargo @CargoArguments
}
exit $LASTEXITCODE
