# Installs iii @VERSION@ from the iii registry, on Windows, with the
# installer iii published for this version.
#   irm https://iii.dev/registry/install/iii/@REQUESTED@.ps1 | iex
# Generated for @REQUESTED@ -> @VERSION@; artefact URLs and SHA-256 digests are pinned.
#
# The registry wrote this preamble: the pinned artefacts and the *-IiiRegistry*
# helpers. The installer iii published follows the line that ends it.
# Both run in one script block, so nothing they define outlives the install.
& {
$III_REGISTRY_URL = 'https://iii.dev/registry'
$III_REGISTRY_NAME = 'iii'
$III_REGISTRY_VERSION = '@VERSION@'
$III_REGISTRY_REQUESTED = '@REQUESTED@'
$III_REGISTRY_TARGETS = @('aarch64-apple-darwin', 'aarch64-pc-windows-msvc', 'aarch64-unknown-linux-gnu', 'armv7-unknown-linux-gnueabihf', 'i686-pc-windows-msvc', 'x86_64-apple-darwin', 'x86_64-pc-windows-msvc', 'x86_64-unknown-linux-gnu', 'x86_64-unknown-linux-musl')
$IiiRegistryArtifacts = @{
  'aarch64-apple-darwin' = @{ Url = 'https://cdn.iii.cloud/workers/iii/@VERSION@/iii-aarch64-apple-darwin.tar.gz'; Sha256 = '@sha256:aarch64-apple-darwin@'; Bin = 'iii'; Format = 'tar.gz' }
  'aarch64-pc-windows-msvc' = @{ Url = 'https://cdn.iii.cloud/workers/iii/@VERSION@/iii-aarch64-pc-windows-msvc.zip'; Sha256 = '@sha256:aarch64-pc-windows-msvc@'; Bin = 'iii'; Format = 'zip' }
  'aarch64-unknown-linux-gnu' = @{ Url = 'https://cdn.iii.cloud/workers/iii/@VERSION@/iii-aarch64-unknown-linux-gnu.tar.gz'; Sha256 = '@sha256:aarch64-unknown-linux-gnu@'; Bin = 'iii'; Format = 'tar.gz' }
  'armv7-unknown-linux-gnueabihf' = @{ Url = 'https://cdn.iii.cloud/workers/iii/@VERSION@/iii-armv7-unknown-linux-gnueabihf.tar.gz'; Sha256 = '@sha256:armv7-unknown-linux-gnueabihf@'; Bin = 'iii'; Format = 'tar.gz' }
  'i686-pc-windows-msvc' = @{ Url = 'https://cdn.iii.cloud/workers/iii/@VERSION@/iii-i686-pc-windows-msvc.zip'; Sha256 = '@sha256:i686-pc-windows-msvc@'; Bin = 'iii'; Format = 'zip' }
  'x86_64-apple-darwin' = @{ Url = 'https://cdn.iii.cloud/workers/iii/@VERSION@/iii-x86_64-apple-darwin.tar.gz'; Sha256 = '@sha256:x86_64-apple-darwin@'; Bin = 'iii'; Format = 'tar.gz' }
  'x86_64-pc-windows-msvc' = @{ Url = 'https://cdn.iii.cloud/workers/iii/@VERSION@/iii-x86_64-pc-windows-msvc.zip'; Sha256 = '@sha256:x86_64-pc-windows-msvc@'; Bin = 'iii'; Format = 'zip' }
  'x86_64-unknown-linux-gnu' = @{ Url = 'https://cdn.iii.cloud/workers/iii/@VERSION@/iii-x86_64-unknown-linux-gnu.tar.gz'; Sha256 = '@sha256:x86_64-unknown-linux-gnu@'; Bin = 'iii'; Format = 'tar.gz' }
  'x86_64-unknown-linux-musl' = @{ Url = 'https://cdn.iii.cloud/workers/iii/@VERSION@/iii-x86_64-unknown-linux-musl.tar.gz'; Sha256 = '@sha256:x86_64-unknown-linux-musl@'; Bin = 'iii'; Format = 'tar.gz' }
}

# The build of a target: an object with Url, Sha256, Bin (the program inside
# the archive) and Format (zip, tar.gz or dmg), or $null when this version has
# no build for it.
function Get-IiiRegistryArtifact {
  param([Parameter(Mandatory = $true)][string]$Target)
  $artifact = $IiiRegistryArtifacts[$Target]
  if ($null -eq $artifact) { return $null }
  return New-Object -TypeName PSObject -Property $artifact
}

# The Windows target this machine installs, the best build this version has for
# its architecture, or $null (not Windows, or no build for it).
function Get-IiiRegistryTarget {
  if ($PSVersionTable.PSEdition -eq 'Core' -and -not $IsWindows) { return $null }
  $arch = $null
  try { $arch = [System.Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString() } catch { $arch = $null }
  if (-not $arch) { $arch = $env:PROCESSOR_ARCHITEW6432 }
  if (-not $arch) { $arch = $env:PROCESSOR_ARCHITECTURE }
  if (-not $arch) { return $null }
  $arch = $arch.ToUpperInvariant()
  $candidates = @()
    if (@('X64', 'AMD64') -contains $arch) { $candidates = @('x86_64-pc-windows-msvc', 'x86_64-pc-windows-gnu') }
    if (@('ARM64') -contains $arch) { $candidates = @('aarch64-pc-windows-msvc', 'x86_64-pc-windows-msvc') }
    if (@('X86') -contains $arch) { $candidates = @('i686-pc-windows-msvc', 'i686-pc-windows-gnu') }
  foreach ($candidate in $candidates) {
    if ($IiiRegistryArtifacts.ContainsKey($candidate)) { return $candidate }
  }
  return $null
}

# Downloads the build of $Target to $Path and checks its SHA-256. On failure it
# removes $Path and throws why.
function Save-IiiRegistryArtifact {
  param([Parameter(Mandatory = $true)][string]$Target, [Parameter(Mandatory = $true)][string]$Path)
  $Path = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Path)
  $artifact = Get-IiiRegistryArtifact $Target
  if ($null -eq $artifact) {
    throw ('{0} {1} has no build for {2} (available: {3})' -f $III_REGISTRY_NAME, $III_REGISTRY_VERSION, $Target, ($III_REGISTRY_TARGETS -join ', '))
  }
  # Windows PowerShell on an older .NET offers no TLS 1.2 unless asked.
  try {
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
  } catch { }
  # The progress bar slows Invoke-WebRequest down many times over on 5.1.
  $ProgressPreference = 'SilentlyContinue'
  $attempt = 0
  while ($true) {
    $attempt += 1
    try {
      Invoke-WebRequest -Uri $artifact.Url -OutFile $Path -UseBasicParsing -ErrorAction Stop
      break
    } catch {
      if ($attempt -ge 3) {
        Remove-Item -LiteralPath $Path -Force -ErrorAction SilentlyContinue
        throw ('could not download {0}: {1}' -f $artifact.Url, $_.Exception.Message)
      }
      Start-Sleep -Seconds 2
    }
  }
  $stream = [IO.File]::OpenRead($Path)
  $sha256 = [Security.Cryptography.SHA256]::Create()
  try { $digest = $sha256.ComputeHash($stream) } finally { $sha256.Dispose(); $stream.Dispose() }
  $actual = -join ($digest | ForEach-Object { $_.ToString('x2') })
  if ($actual -ne $artifact.Sha256) {
    Remove-Item -LiteralPath $Path -Force -ErrorAction SilentlyContinue
    throw ('checksum mismatch for {0} (expected {1}, got {2})' -f $Target, $artifact.Sha256, $actual)
  }
}

# Extracts $Archive into $Destination: a .zip with .NET's ZipFile, a .tar.gz or
# .tgz with tar (Windows 10 1803 and later ship it). Throws when it cannot.
function Expand-IiiRegistryArchive {
  param([Parameter(Mandatory = $true)][string]$Archive, [Parameter(Mandatory = $true)][string]$Destination)
  $Archive = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Archive)
  $Destination = $ExecutionContext.SessionState.Path.GetUnresolvedProviderPathFromPSPath($Destination)
  [IO.Directory]::CreateDirectory($Destination) | Out-Null
  if ($Archive.EndsWith('.zip', [StringComparison]::OrdinalIgnoreCase)) {
    Add-Type -AssemblyName System.IO.Compression.FileSystem
    [IO.Compression.ZipFile]::ExtractToDirectory($Archive, $Destination)
  } elseif ($Archive.EndsWith('.tar.gz', [StringComparison]::OrdinalIgnoreCase) -or $Archive.EndsWith('.tgz', [StringComparison]::OrdinalIgnoreCase)) {
    & tar -xzf $Archive -C $Destination
    if ($LASTEXITCODE -ne 0) { throw ('could not extract {0} (tar exited with {1})' -f $Archive, $LASTEXITCODE) }
  } else {
    throw ('cannot extract {0}: not a .zip or a .tar.gz' -f $Archive)
  }
}

# Puts $Dir first on the user's Path and on this session's, unless it is
# already there. The user's Path stays an expandable string (%VARS% kept).
# Returns $true when it changed the user's Path.
function Add-IiiRegistryUserPath {
  param([Parameter(Mandatory = $true)][string]$Dir)
  $separator = [IO.Path]::PathSeparator
  if (@($env:Path -split $separator) -notcontains $Dir) { $env:Path = $Dir + $separator + $env:Path }
  if (-not (Test-Path -LiteralPath 'HKCU:\Environment')) { return $false }
  $key = Get-Item -LiteralPath 'HKCU:\Environment'
  $current = $key.GetValue('Path', '', 'DoNotExpandEnvironmentNames')
  $entries = @($current -split ';' | Where-Object { $_ })
  if ($entries -contains $Dir) { return $false }
  Set-ItemProperty -LiteralPath 'HKCU:\Environment' -Name 'Path' -Value ((@($Dir) + $entries) -join ';') -Type ExpandString
  # Setting a user variable through .NET tells running programs (Explorer, new
  # terminals) that the environment changed; this one is removed again.
  [Environment]::SetEnvironmentVariable('III_REGISTRY_PATH_CHANGED', '1', 'User')
  [Environment]::SetEnvironmentVariable('III_REGISTRY_PATH_CHANGED', $null, 'User')
  return $true
}
# --- end of the registry preamble ---
