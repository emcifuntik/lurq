# The pipeline's Check: the debug suite, the suite of the profiling features, the guard that only measures release frames,
# then a build of the tests with the features of both suites together.
$ErrorActionPreference = 'Stop'
$workspace = Split-Path -Parent $PSScriptRoot
$commands = @(
  @('test', '-p', 'lurq', '--features', 'markdown,canvas,query,tokio,svg,image,screenshot,form,router,i18n', '--no-fail-fast'),
  # The profiler, DevTools and MCP are compiled out of the suite above.
  @('test', '-p', 'lurq', '--features', 'devtools,mcp,perf_profile,counting_allocator', '--no-fail-fast'),
  @('test', '-p', 'lurq', '--release', '--test', 'layout_stack_depth'),
  # The size budgets (`size_budget.rs`) are checked when the tests are built, and sizes grow with features: neither
  # suite above turns on the features of the other.
  @('check', '-p', 'lurq', '--tests', '--features', 'markdown,canvas,query,tokio,svg,image,screenshot,form,router,i18n,devtools,mcp,perf_profile,counting_allocator')
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
