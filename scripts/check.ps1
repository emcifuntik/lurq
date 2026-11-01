# The pipeline's Check: the debug suite, then the guard that only measures release frames.
$ErrorActionPreference = 'Stop'
$workspace = Split-Path -Parent $PSScriptRoot
$commands = @(
  @('test', '-p', 'lurq', '--features', 'markdown,canvas,query,tokio,svg,image,screenshot,form,router,i18n', '--no-fail-fast'),
  @('test', '-p', 'lurq', '--release', '--test', 'layout_stack_depth')
)
Push-Location -LiteralPath $workspace
try {
  foreach ($arguments in $commands) {
    Write-Host "> cargo $($arguments -join ' ')"
    & cargo @arguments
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
  }
} finally {
  Pop-Location
}
