param(
  [ValidateSet('debug', 'release')]
  [string]$Configuration = 'debug',
  [switch]$RequireStagedResource
)

$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$companion = Join-Path $projectRoot "src-tauri\target\$Configuration\lightr-ask-mcp.exe"
if (-not (Test-Path -LiteralPath $companion -PathType Leaf)) {
  throw "Companion MCP artifact is missing: $companion"
}

$targets = @($companion)
if ($RequireStagedResource) {
  $stagedCompanion = Join-Path $projectRoot 'src-tauri\binaries\lightr-ask-mcp.exe'
  if (-not (Test-Path -LiteralPath $stagedCompanion -PathType Leaf)) {
    throw "Bundled companion staging artifact is missing: $stagedCompanion"
  }
  if ((Get-FileHash -Algorithm SHA256 -LiteralPath $companion).Hash -ne (Get-FileHash -Algorithm SHA256 -LiteralPath $stagedCompanion).Hash) {
    throw "Bundled companion staging artifact does not match the release companion."
  }
  $targets += $stagedCompanion
}

# The companion does not contact the socket for tools/list. Keep the dummy
# endpoint/token local and non-secret so this is safe for CI logs.
$request = '{"jsonrpc":"2.0","id":"smoke-tools-list","method":"tools/list","params":{}}'
$expected = @('ask_user_question', 'delegate_to_agent', 'get_delegation_status', 'cancel_delegation')
foreach ($target in $targets) {
  $line = @($request | & $target --socket 127.0.0.1:9 --token smoke --conn-id smoke --delegation-enabled true | Select-Object -First 1)[0]
  if ([string]::IsNullOrWhiteSpace($line)) {
    throw "Companion MCP did not return a tools/list response: $target"
  }
  $response = $line | ConvertFrom-Json
  $names = @($response.result.tools | ForEach-Object { $_.name })
  $missing = @($expected | Where-Object { $_ -notin $names })
  if ($missing.Count -gt 0) {
    throw "Companion MCP tools/list is missing from ${target}: $($missing -join ', ')"
  }
  Write-Host "Companion MCP smoke passed ($Configuration): $target ($($names -join ', '))"
}
