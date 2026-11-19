# The iii engine's Windows installer on the iii registry:
#
#   irm https://iii.dev/registry/install/iii/latest.ps1 | iex
#
# Every iii release publishes this file with its binaries (`registry publish
# --install-script-ps1`, see .github/workflows/_publish-engine-registry.yml),
# next to install.sh, the one for macOS and Linux. The registry serves it after
# a preamble that pins that version's archives and defines, for this script:
#
#   $III_REGISTRY_URL        the registry site, e.g. https://iii.dev/registry
#   $III_REGISTRY_NAME       iii
#   $III_REGISTRY_VERSION    the version this script installs
#   $III_REGISTRY_REQUESTED  the tag or version in the URL (latest, rc, 0.27.0)
#   $III_REGISTRY_TARGETS    the targets that version has builds for
#   Get-IiiRegistryArtifact <target>            the pinned archive of a target, or $null
#   Get-IiiRegistryTarget                       this machine's Windows target, or $null
#   Save-IiiRegistryArtifact <target> <path>    download it and check its SHA-256, or throw
#   Expand-IiiRegistryArchive <archive> <dir>   extract a .zip or a .tar.gz
#   Add-IiiRegistryUserPath <dir>               put a folder on the user's Path
#
# Both run in one script block (`& { ... }`), so nothing defined here outlives
# the install. `irm | iex` passes no arguments, so the options are environment
# variables:
#
#   VERSION               the version or tag to install instead (0.27.0, rc)
#   III_RELEASE_TAG       the version of a GitHub release tag (iii/v0.27.0)
#   BIN_DIR               where iii.exe goes (default: $PREFIX\bin if PREFIX is
#                         set, else ~\.local\bin)
#   PREFIX                installation prefix (default: ~\.local)
#   TARGET                another build's target triple (aarch64-pc-windows-msvc)
#   III_NON_INTERACTIVE   any value: install only, and print the setup command
#                         instead of offering to run it
#   NO_MODIFY_PATH        any value: leave the user's Path alone
#
# Like the registry's own installers it runs on Windows PowerShell 5.1 (what
# `irm | iex` gets on a stock Windows) and PowerShell 7: no `??`, ternaries,
# `&&` or `||`, ASCII only, and failures `throw`, never `exit`, which under
# `iex` would close the user's window.
#
# See https://github.com/iii-hq/registry/blob/main/docs/EXECUTABLES.md#powershell-installers-windows
# The tests compose it with engine/tests/fixtures/registry-preamble.ps1.

$ErrorActionPreference = 'Stop'

$BinName = 'iii'
$ExeName = 'iii.exe'
# What the failure path reports: the binary that sends the event, install or
# upgrade, and the versions. A hashtable, so the functions below can update it.
$state = @{ Emitter = $null; Prefix = $null; From = ''; Release = '' }

# ---------------------------------------------------------------------------
# Native commands
# ---------------------------------------------------------------------------

# One argument quoted for a Windows command line (the rules CommandLineToArgvW
# and the C runtime read back). Windows PowerShell passes embedded quotes to
# native programs unescaped, which breaks the JSON of the telemetry events, so
# the processes below are started with a command line built here.
function ConvertTo-IiiArgument {
  param([string]$Value)
  if ($Value.Length -gt 0 -and $Value -notmatch '[\s"]') { return $Value }
  $escaped = $Value -replace '(\\*)"', '$1$1\"'
  $escaped = $escaped -replace '(\\+)$', '$1$1'
  return '"' + $escaped + '"'
}

# Runs $Exe with $Arguments, without a window and without reading the console.
# Returns @{ Code; Stdout }, or $null when it cannot be started (a build for
# another architecture). With -NoWait it returns as soon as the process runs.
function Invoke-IiiNative {
  param([string]$Exe, [string[]]$Arguments, [int]$TimeoutMs = 30000, [switch]$NoWait)
  $info = New-Object System.Diagnostics.ProcessStartInfo
  $info.FileName = $Exe
  $info.Arguments = (@($Arguments) | ForEach-Object { ConvertTo-IiiArgument $_ }) -join ' '
  $info.UseShellExecute = $false
  $info.CreateNoWindow = $true
  $info.RedirectStandardInput = $true
  $info.RedirectStandardOutput = $true
  $info.RedirectStandardError = $true
  try {
    $process = [System.Diagnostics.Process]::Start($info)
  } catch {
    return $null
  }
  $process.StandardInput.Close()
  if ($NoWait) { return @{ Code = $null; Stdout = '' } }
  $stdout = $process.StandardOutput.ReadToEndAsync()
  $null = $process.StandardError.ReadToEndAsync()
  if (-not $process.WaitForExit($TimeoutMs)) {
    try { $process.Kill() } catch { }
    return @{ Code = -1; Stdout = '' }
  }
  return @{ Code = $process.ExitCode; Stdout = $stdout.Result }
}

# The version `<exe> --version` prints (its last word), or '' when it does not
# run or prints nothing.
function Get-IiiVersion {
  param([string]$Exe)
  if (-not $Exe -or -not (Test-Path -LiteralPath $Exe -PathType Leaf)) { return '' }
  $run = Invoke-IiiNative $Exe @('--version') 10000
  if ($null -eq $run -or $run.Code -ne 0) { return '' }
  $words = @(($run.Stdout -split '\s+') | Where-Object { $_ })
  if ($words.Count -eq 0) { return '' }
  return $words[$words.Count - 1]
}

# The iii on PATH, or $null.
function Find-IiiOnPath {
  $found = Get-Command $BinName -CommandType Application -ErrorAction SilentlyContinue | Select-Object -First 1
  if ($found) { return $found.Source }
  return $null
}

# The registry ref (a version or a tag) that a VERSION or III_RELEASE_TAG value
# names: `iii/v0.27.0`, `v0.27.0` and `0.27.0` all name 0.27.0, and the GitHub
# tag `iii-alpha/v0.27.0-alpha.1` names 0.27.0-alpha.1.
function ConvertTo-IiiRegistryRef {
  param([string]$Value)
  $ref = ($Value -split '/')[-1]
  if ($ref.StartsWith('v')) { $ref = $ref.Substring(1) }
  return $ref
}

# ---------------------------------------------------------------------------
# Telemetry
# ---------------------------------------------------------------------------

# Sends one install event through $state.Emitter (an iii that knows
# --install-only-generate-ids); nothing without one. -Background returns at
# once: the binary sends each event on its own and gives up after 3 s.
function Send-IiiEvent {
  param([string]$Type, $Properties, [switch]$Background)
  $emitter = $state.Emitter
  if (-not $emitter -or -not (Test-Path -LiteralPath $emitter -PathType Leaf)) { return }
  $json = ConvertTo-Json -InputObject $Properties -Compress
  $arguments = @('--install-only-generate-ids', '--install-event-type', $Type, '--install-event-properties', $json)
  try {
    if ($Background) {
      $null = Invoke-IiiNative $emitter $arguments -NoWait
    } else {
      $null = Invoke-IiiNative $emitter $arguments 10000
    }
  } catch { }
}

# Reports the failure ($Stage, $Message) when an install or upgrade is under
# way, then stops the installer with $Message.
function Stop-IiiInstall {
  param([string]$Stage, [string]$Message)
  if ($state.Prefix -eq 'upgrade') {
    Send-IiiEvent 'upgrade_failed' ([ordered]@{
        from_version = $state.From; to_version = $state.Release; install_method = 'powershell'
        target_binary = $BinName; error_stage = $Stage; error_message = $Message
      })
  } elseif ($state.Prefix -eq 'install') {
    Send-IiiEvent 'install_failed' ([ordered]@{
        install_method = 'powershell'; target_binary = $BinName; error_stage = $Stage; error_message = $Message
      })
  }
  throw $Message
}

# ---------------------------------------------------------------------------
# Without the registry's preamble there is nothing to install
# ---------------------------------------------------------------------------

if (-not $III_REGISTRY_VERSION -or -not (Get-Command Save-IiiRegistryArtifact -ErrorAction SilentlyContinue)) {
  throw "this is the body of the registry's iii installer; run it from the registry: irm https://iii.dev/registry/install/iii/latest.ps1 | iex"
}
$state.Release = $III_REGISTRY_VERSION
$nonInteractive = [bool]$env:III_NON_INTERACTIVE

# ---------------------------------------------------------------------------
# Release selection
# ---------------------------------------------------------------------------

# The registry pinned one version in this script. Asking for another one
# (VERSION or III_RELEASE_TAG) runs the registry's script for it instead, in
# this same session, with III_INSTALL_REDIRECTED naming what was asked for.
# That script pins it, so it never goes on to a third, and an exact version
# it does not pin stops the install.
if ($env:III_INSTALL_REDIRECTED -match '^[0-9]+\.[0-9]+\.[0-9]+' -and $env:III_INSTALL_REDIRECTED -ne $III_REGISTRY_VERSION) {
  Stop-IiiInstall 'download' ('the registry served {0} {1} for {2}' -f $BinName, $III_REGISTRY_VERSION, $env:III_INSTALL_REDIRECTED)
}
$requestedRef = ''
if ($env:III_RELEASE_TAG) {
  $requestedRef = ConvertTo-IiiRegistryRef $env:III_RELEASE_TAG
} elseif ($env:VERSION) {
  $requestedRef = ConvertTo-IiiRegistryRef $env:VERSION
}

if ($requestedRef -and $requestedRef -ne $III_REGISTRY_VERSION -and $requestedRef -ne $III_REGISTRY_REQUESTED) {
  if ($env:III_INSTALL_REDIRECTED) {
    Stop-IiiInstall 'download' ('the registry served {0} {1} for {2}' -f $BinName, $III_REGISTRY_VERSION, $requestedRef)
  }
  $url = '{0}/install/{1}/{2}.ps1' -f $III_REGISTRY_URL, $III_REGISTRY_NAME, $requestedRef
  Write-Host ('installing {0} {1} from {2}' -f $BinName, $requestedRef, $III_REGISTRY_URL)
  try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
  } catch { }
  $ProgressPreference = 'SilentlyContinue'
  try {
    $next = (Invoke-WebRequest -Uri $url -UseBasicParsing -ErrorAction Stop).Content
  } catch {
    Stop-IiiInstall 'download' ('{0} {1} is not in the registry ({2})' -f $BinName, $requestedRef, $url)
  }
  if ($next -is [byte[]]) { $next = [Text.Encoding]::UTF8.GetString($next) }
  $saved = @{ VERSION = $env:VERSION; III_RELEASE_TAG = $env:III_RELEASE_TAG; III_INSTALL_REDIRECTED = $env:III_INSTALL_REDIRECTED }
  $env:VERSION = $null
  $env:III_RELEASE_TAG = $null
  $env:III_INSTALL_REDIRECTED = $requestedRef
  try {
    Invoke-Expression $next
  } finally {
    $env:VERSION = $saved.VERSION
    $env:III_RELEASE_TAG = $saved.III_RELEASE_TAG
    $env:III_INSTALL_REDIRECTED = $saved.III_INSTALL_REDIRECTED
  }
  return
}

# ---------------------------------------------------------------------------
# Target
# ---------------------------------------------------------------------------

$onWindows = $PSVersionTable.PSEdition -ne 'Core' -or $IsWindows
if ($env:TARGET) {
  $target = $env:TARGET
} else {
  if (-not $onWindows) {
    throw ('this is the Windows installer; on macOS and Linux run: curl -fsSL {0}/install/{1}/{2}.sh | sh' -f $III_REGISTRY_URL, $III_REGISTRY_NAME, $III_REGISTRY_REQUESTED)
  }
  $target = Get-IiiRegistryTarget
  if (-not $target) {
    Stop-IiiInstall 'platform' ('{0} v{1} has no build for Windows on {2} (set TARGET=<triple> to override; builds: {3})' -f $BinName, $III_REGISTRY_VERSION, $env:PROCESSOR_ARCHITECTURE, ($III_REGISTRY_TARGETS -join ', '))
  }
}

if ($III_REGISTRY_REQUESTED -eq $III_REGISTRY_VERSION) {
  Write-Host ('installing {0} v{1}' -f $BinName, $III_REGISTRY_VERSION)
} else {
  Write-Host ('installing {0} v{1} ({2})' -f $BinName, $III_REGISTRY_VERSION, $III_REGISTRY_REQUESTED)
}

$artifact = Get-IiiRegistryArtifact $target
if ($null -eq $artifact) {
  $lines = @('', ('{0} v{1} does not include a prebuilt binary for {2}.' -f $BinName, $III_REGISTRY_VERSION, $target))
  if ($III_REGISTRY_TARGETS.Count -gt 0) {
    $lines += ''
    $lines += ('Available {0} targets in v{1}:' -f $BinName, $III_REGISTRY_VERSION)
    foreach ($t in $III_REGISTRY_TARGETS) { $lines += ('  - ' + $t) }
  }
  $lines += ''
  $lines += 'Try:'
  $lines += ('  - Install a stable release:  irm {0}/install/{1}/latest.ps1 | iex' -f $III_REGISTRY_URL, $III_REGISTRY_NAME)
  $lines += ('  - Pin a different version:   $env:VERSION = ''<x.y.z>''; irm {0}/install/{1}/latest.ps1 | iex' -f $III_REGISTRY_URL, $III_REGISTRY_NAME)
  $lines += ('  - Override target triple:    $env:TARGET = ''<triple>''; irm {0}/install/{1}/latest.ps1 | iex' -f $III_REGISTRY_URL, $III_REGISTRY_NAME)
  foreach ($line in $lines) { [Console]::Error.WriteLine($line) }
  Stop-IiiInstall 'download' ('no release asset found for target: {0}' -f $target)
}
$assetName = Split-Path -Leaf $artifact.Url

# ---------------------------------------------------------------------------
# Install directory, and upgrade or fresh install
# ---------------------------------------------------------------------------

if ($env:BIN_DIR) {
  $binDir = $env:BIN_DIR
} elseif ($env:PREFIX) {
  $binDir = Join-Path $env:PREFIX 'bin'
} else {
  $binDir = Join-Path (Join-Path $HOME '.local') 'bin'
}
$binDir = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($binDir)
$dest = Join-Path $binDir $ExeName

$state.From = Get-IiiVersion $dest
# Checked before the install: an upgrade gets no setup offer.
$hadIii = [bool]$state.From -or [bool](Get-IiiVersion (Find-IiiOnPath))
if ($state.From) {
  $state.Prefix = 'upgrade'
  $state.Emitter = $dest
} else {
  $state.Prefix = 'install'
}

# Idempotency: already at this version, nothing to download and nothing to
# announce.
if ($state.From -and $state.From -eq $III_REGISTRY_VERSION) {
  Write-Host ('{0} is already at v{1} - nothing to do' -f $BinName, $III_REGISTRY_VERSION)
  return
}

New-Item -ItemType Directory -Path $binDir -Force | Out-Null
# What an earlier upgrade could not delete while that iii was running.
Get-ChildItem -LiteralPath $binDir -Filter ('.{0}.*.old.exe' -f $BinName) -Force -ErrorAction SilentlyContinue |
  ForEach-Object { Remove-Item -LiteralPath $_.FullName -Force -ErrorAction SilentlyContinue }

$tmp = Join-Path ([IO.Path]::GetTempPath()) ('iii-install-' + [Guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $tmp | Out-Null
$staged = Join-Path $binDir ('.{0}.{1}.new.exe' -f $BinName, $PID)
try {
  # -------------------------------------------------------------------------
  # Download, and check the SHA-256 the registry pinned
  # -------------------------------------------------------------------------

  Write-Host ('downloading {0}...' -f $assetName)
  $archive = Join-Path $tmp $assetName
  try {
    Save-IiiRegistryArtifact $target $archive
  } catch {
    Stop-IiiInstall 'download' ('failed to download or verify {0}: {1}' -f $assetName, $_.Exception.Message)
  }

  $extracted = Join-Path $tmp 'x'
  try {
    Expand-IiiRegistryArchive $archive $extracted
  } catch {
    Stop-IiiInstall 'extract' ('failed to extract {0}: {1}' -f $assetName, $_.Exception.Message)
  }

  $binFile = $null
  foreach ($name in @($ExeName, $BinName)) {
    $candidate = Join-Path $extracted $name
    if (Test-Path -LiteralPath $candidate -PathType Leaf) { $binFile = $candidate; break }
  }
  if (-not $binFile) {
    $binFile = Get-ChildItem -LiteralPath $extracted -Recurse -File |
      Where-Object { $_.Name -eq $ExeName -or $_.Name -eq $BinName } |
      Select-Object -First 1 -ExpandProperty FullName
  }
  if (-not $binFile) { Stop-IiiInstall 'binary_lookup' 'binary not found in downloaded asset' }

  # -------------------------------------------------------------------------
  # Verify: the new binary runs here
  # -------------------------------------------------------------------------

  # Asked before anything is replaced, so a build that cannot run on this
  # machine (another architecture) never takes the place of an iii that can.
  # An explicit TARGET may name another machine's build on purpose, so that one
  # is installed with a warning. It runs from a copy beside its destination,
  # and a rename in the same folder then puts it in place.
  try {
    Copy-Item -LiteralPath $binFile -Destination $staged -Force
  } catch {
    Stop-IiiInstall 'install' ('failed to write {0}: {1}' -f $staged, $_.Exception.Message)
  }
  $newVersion = Get-IiiVersion $staged
  if (-not $newVersion -and -not $env:TARGET) {
    Remove-Item -LiteralPath $staged -Force -ErrorAction SilentlyContinue
    if ($state.From) {
      Stop-IiiInstall 'verify' ('the downloaded {0} v{1} ({2}) does not run on this system; {3} v{4} is unchanged' -f $BinName, $III_REGISTRY_VERSION, $target, $dest, $state.From)
    }
    Stop-IiiInstall 'verify' ('the downloaded {0} v{1} ({2}) does not run on this system; nothing was installed (set TARGET=<triple> to pick another build: {3})' -f $BinName, $III_REGISTRY_VERSION, $target, ($III_REGISTRY_TARGETS -join ', '))
  }

  # -------------------------------------------------------------------------
  # Install
  # -------------------------------------------------------------------------

  $state.Emitter = $binFile
  # Best-effort: the binary initializes its telemetry IDs, in the foreground and
  # with no network, before any event is sent beside the install. Older
  # binaries do not know the flag.
  $null = Invoke-IiiNative $binFile @('--install-only-generate-ids') 10000
  $arch = ''
  if ($env:PROCESSOR_ARCHITECTURE) { $arch = $env:PROCESSOR_ARCHITECTURE.ToLowerInvariant() }
  if ($state.Prefix -eq 'upgrade') {
    Send-IiiEvent 'upgrade_started' ([ordered]@{
        from_version = $state.From; to_version = $III_REGISTRY_VERSION; install_method = 'powershell'; target_binary = $BinName
      }) -Background
  } else {
    Send-IiiEvent 'install_started' ([ordered]@{
        install_method = 'powershell'; target_binary = $BinName; os = 'windows'; arch = $arch
      }) -Background
  }

  # A running iii.exe cannot be overwritten, but it can be renamed: the old one
  # moves aside, the new one takes its name, and the old one goes when it can.
  $old = $null
  try {
    if (Test-Path -LiteralPath $dest) {
      $old = Join-Path $binDir ('.{0}.{1}.old.exe' -f $BinName, $PID)
      [IO.File]::Move($dest, $old)
    }
    [IO.File]::Move($staged, $dest)
  } catch {
    if ($old -and -not (Test-Path -LiteralPath $dest) -and (Test-Path -LiteralPath $old)) {
      try { [IO.File]::Move($old, $dest) } catch { }
    }
    Stop-IiiInstall 'install' ('failed to install binary to {0}: {1}' -f $dest, $_.Exception.Message)
  }
  if ($old) { Remove-Item -LiteralPath $old -Force -ErrorAction SilentlyContinue }

  $installedVersion = Get-IiiVersion $dest
  $shown = $installedVersion
  if (-not $shown) { $shown = $III_REGISTRY_VERSION }
  if ($state.Prefix -eq 'upgrade') {
    Write-Host ('upgraded {0}: v{1} -> v{2}' -f $BinName, $state.From, $shown)
  } else {
    Write-Host ('installed {0} v{1} to {2}' -f $BinName, $shown, $dest)
  }

  # -------------------------------------------------------------------------
  # Telemetry: success, from the installed binary
  # -------------------------------------------------------------------------

  $state.Emitter = $dest
  if ($state.Prefix -eq 'upgrade') {
    Send-IiiEvent 'upgrade_succeeded' ([ordered]@{
        from_version = $state.From; to_version = $installedVersion; install_method = 'powershell'; target_binary = $BinName
      }) -Background
  } else {
    Send-IiiEvent 'install_succeeded' ([ordered]@{
        installed_version = $installedVersion; install_method = 'powershell'; target_binary = $BinName
      }) -Background
  }

  # Only an explicit TARGET gets here with a binary that does not run here.
  if (-not $installedVersion) {
    Write-Warning ("{0} installed but 'iii --version' produced no output: the {1} build may not run on this system (try: {2} --version)" -f $BinName, $target, $dest)
  }
} finally {
  Remove-Item -LiteralPath $tmp -Recurse -Force -ErrorAction SilentlyContinue
  Remove-Item -LiteralPath $staged -Force -ErrorAction SilentlyContinue
}

# ---------------------------------------------------------------------------
# Path
# ---------------------------------------------------------------------------

if ($env:NO_MODIFY_PATH) {
  if (@($env:Path -split [IO.Path]::PathSeparator) -notcontains $binDir) {
    Write-Host ''
    Write-Host ('add {0} to your Path to run {1} from any terminal' -f $binDir, $BinName)
  }
} elseif (Add-IiiRegistryUserPath $binDir) {
  Write-Host ''
  Write-Host ('added {0} to your Path: this terminal and the ones you open from now on find {1}' -f $binDir, $BinName)
}

# ---------------------------------------------------------------------------
# Onboarding: start the harness
# ---------------------------------------------------------------------------

# An upgrade already has iii set up, so it gets no setup offer.
if ($hadIii) { return }

$startCmd = '{0} project init --learn-iii' -f $BinName
$quickstartUrl = 'https://iii.dev/docs/quickstart'

# Does the binary just installed know `--learn-iii`? Ask its parser: `--help`
# short-circuits it, so an accepted flag prints help and scaffolds nothing.
$probe = Invoke-IiiNative $dest @('project', 'init', '--learn-iii', '--help') 10000
$interactive = -not $nonInteractive -and [Environment]::UserInteractive -and -not [Console]::IsInputRedirected -and -not [Console]::IsOutputRedirected
if ($null -eq $probe -or $probe.Code -ne 0) {
  # Never offer what this binary cannot run, and never name the command.
  Write-Host ''
  Write-Host ("If you're new to iii, get started quickly here: {0}" -f $quickstartUrl)
} elseif ($interactive) {
  Write-Host ''
  Write-Host "Now let's setup your first iii project [Press Enter to Continue] " -NoNewline
  # Ctrl+C here stops the install with iii already in place.
  $null = [Console]::ReadLine()
  & $dest project init --learn-iii
} else {
  Write-Host ''
  Write-Host 'To start the iii harness and see what iii can do, run:'
  Write-Host ('  ' + $startCmd)
}
