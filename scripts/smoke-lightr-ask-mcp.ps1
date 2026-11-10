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

# MCP stdio is UTF-8 by spec. Windows PowerShell 5.1 decodes native command
# output with the console code page instead (936 on zh-CN), which turns each
# multi-byte character into mojibake and can consume a following '{' / '}' as a
# double-byte trail byte - ConvertFrom-Json then fails deep inside the payload.
# Launch the companion directly and force UTF-8 decoding of its stdout.
function Invoke-Companion {
  param(
    [Parameter(Mandatory)][string]$Path,
    [Parameter(Mandatory)][string]$Payload
  )

  $psi = New-Object System.Diagnostics.ProcessStartInfo
  $psi.FileName = $Path
  $psi.Arguments = '--socket 127.0.0.1:9 --token smoke --conn-id smoke --delegation-enabled true'
  $psi.UseShellExecute = $false
  $psi.RedirectStandardInput = $true
  $psi.RedirectStandardOutput = $true
  $psi.StandardOutputEncoding = New-Object System.Text.UTF8Encoding $false

  # Windows PowerShell 5.1 builds Process.StandardInput from Console.InputEncoding
  # and .NET Framework has no StandardInputEncoding knob. When that encoding emits
  # a BOM, the companion's UTF-8 line reader rejects the first line and silently
  # drops the request, so tools/list comes back empty. Force a BOM-less UTF-8
  # writer for this call; PowerShell 7 already defaults to BOM-less UTF-8.
  $previousInputEncoding = [Console]::InputEncoding
  try {
    [Console]::InputEncoding = New-Object System.Text.UTF8Encoding $false
  } catch {
    # Hosts without a console may refuse the change; keep the host default.
  }

  try {
    $process = [System.Diagnostics.Process]::Start($psi)
    try {
      $process.StandardInput.WriteLine($Payload)
      $process.StandardInput.Close()
      $output = $process.StandardOutput.ReadToEnd()
      if (-not $process.WaitForExit(30000)) {
        $process.Kill()
        throw "Companion MCP did not exit after stdin closed: $Path"
      }
    } finally {
      $process.Dispose()
    }
  } finally {
    [Console]::InputEncoding = $previousInputEncoding
  }

  $lines = @($output -split "`r?`n" | Where-Object { -not ([string]::IsNullOrWhiteSpace($_)) })
  if ($lines.Count -eq 0) { return $null }
  return $lines[0]
}

foreach ($target in $targets) {
  $line = Invoke-Companion -Path $target -Payload $request
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
