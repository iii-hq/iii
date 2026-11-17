// Tests for engine/registry/install.ps1, the iii installer the registry serves
// for Windows (`irm https://iii.dev/registry/install/iii/latest.ps1 | iex`).
//
//   node --test engine/tests/registry_install_ps1.test.mjs
//
// The registry serves that file after a preamble it renders per version
// (`renderCustomPowerShellInstallScript` in iii-hq/registry,
// api/src/lib/install-script-ps1.ts). fixtures/registry-preamble.ps1 is that
// preamble, rendered for all nine iii targets with placeholders: @VERSION@,
// @REQUESTED@, and @sha256:<target>@ for each digest. `publish` below fills
// them in for archives it builds, served by a local HTTP server, so the
// scripts run here exactly as the registry composes them (preamble, a blank
// line, the body, the closing brace).
//
// They run under every PowerShell on the machine: pwsh, plus Windows
// PowerShell 5.1 on Windows. Off Windows the fake iii is a shell script and
// each run sets TARGET (the preamble finds no Windows target there); on
// Windows it is a small program compiled with Windows PowerShell, and the
// target is detected. When the registry changes its preamble, render the
// fixture again from it.

import { spawn, spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { chmodSync, existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { createServer } from 'node:http'
import { tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { after, before, describe, it } from 'node:test'
import assert from 'node:assert/strict'
import { fileURLToPath } from 'node:url'

const HERE = dirname(fileURLToPath(import.meta.url))
const BODY = readFileSync(join(HERE, '..', 'registry', 'install.ps1'), 'utf8')
const PREAMBLE = readFileSync(join(HERE, 'fixtures', 'registry-preamble.ps1'), 'utf8')
const ON_WINDOWS = process.platform === 'win32'
const WINDOWS_TARGET = 'x86_64-pc-windows-msvc'

/** Every PowerShell on this machine, as absolute paths: the runs below set their own PATH. */
function shells() {
  const candidates = [process.env.PWSH || 'pwsh', ...(ON_WINDOWS ? ['powershell.exe'] : [])]
  return candidates.flatMap((shell) => {
    const res = spawnSync(
      shell,
      ['-NoProfile', '-NonInteractive', '-Command', '[Diagnostics.Process]::GetCurrentProcess().MainModule.FileName'],
      { encoding: 'utf8' },
    )
    return res.status === 0 ? [res.stdout.trim()] : []
  })
}
const SHELLS = shells()

/**
 * PATH for a run: the system's own folders and the shell's, so no iii
 * installed on this machine counts as one already there.
 */
function hermeticPath(shell) {
  if (!ON_WINDOWS) return `${dirname(shell)}:/usr/bin:/bin`
  const root = process.env.SystemRoot || 'C:\\Windows'
  return [dirname(shell), join(root, 'System32'), root, join(root, 'System32', 'WindowsPowerShell', 'v1.0')].join(';')
}

const ROOT = mkdtempSync(join(tmpdir(), 'iii-install-ps1-'))
let base = ''
let server
const served = []

before(async () => {
  server = createServer((req, res) => {
    served.push(req.url)
    const file = join(ROOT, 'www', decodeURIComponent(new URL(req.url, 'http://x').pathname))
    if (!existsSync(file)) {
      res.writeHead(404)
      res.end('not found')
      return
    }
    res.writeHead(200, { 'content-type': file.endsWith('.ps1') ? 'text/plain; charset=utf-8' : 'application/zip' })
    res.end(readFileSync(file))
  })
  await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
  base = `http://127.0.0.1:${server.address().port}`
})

after(async () => {
  await new Promise((resolve) => server.close(resolve))
  rmSync(ROOT, { recursive: true, force: true })
})

// ------------------------------------------------------------------ fake iii

const compiled = new Map()

/**
 * A fake iii of `version`: prints `iii <version>` for --version, accepts the
 * setup offer's probe, and logs every telemetry call to $III_FAKE_LOG as
 * `<version>|arg|arg...`. `broken`: prints nothing and fails, as a build for
 * another architecture does.
 */
function fakeIii(version, file, { broken = false } = {}) {
  mkdirSync(dirname(file), { recursive: true })
  if (!ON_WINDOWS) {
    const script = broken
      ? '#!/bin/sh\nexit 126\n'
      : `#!/bin/sh
case "$1" in
  --version) echo "iii ${version}" ;;
  --install-only-generate-ids)
    if [ $# -gt 1 ]; then
      line="${version}"
      for a in "$@"; do line="$line|$a"; done
      printf '%s\\n' "$line" >> "$III_FAKE_LOG"
    fi ;;
esac
exit 0
`
    writeFileSync(file, script)
    chmodSync(file, 0o755)
    return
  }
  const key = broken ? 'broken' : version
  if (!compiled.has(key)) {
    const exe = join(ROOT, 'compiled', `${key}.exe`)
    mkdirSync(dirname(exe), { recursive: true })
    const source = broken
      ? 'public static class FakeIii { public static int Main(string[] args) { return 126; } }'
      : `using System; using System.IO;
public static class FakeIii {
  public static int Main(string[] args) {
    if (args.Length > 0 && args[0] == "--version") { Console.WriteLine("iii ${version}"); return 0; }
    if (args.Length > 1 && args[0] == "--install-only-generate-ids") {
      File.AppendAllText(Environment.GetEnvironmentVariable("III_FAKE_LOG"), "${version}|" + string.Join("|", args) + "\\n");
    }
    return 0;
  }
}`
    const src = join(ROOT, 'compiled', `${key}.cs`)
    writeFileSync(src, source)
    const res = spawnSync(
      'powershell.exe',
      [
        '-NoProfile',
        '-NonInteractive',
        '-Command',
        `Add-Type -TypeDefinition ([IO.File]::ReadAllText('${src}')) -OutputAssembly '${exe}' -OutputType ConsoleApplication`,
      ],
      { encoding: 'utf8' },
    )
    assert.equal(res.status, 0, `${res.stdout}${res.stderr}`)
    compiled.set(key, exe)
  }
  writeFileSync(file, readFileSync(compiled.get(key)))
}

function zipDirectory(dir, zip) {
  rmSync(zip, { force: true })
  const res = spawnSync(
    SHELLS[0],
    [
      '-NoProfile',
      '-NonInteractive',
      '-Command',
      `Add-Type -AssemblyName System.IO.Compression.FileSystem; [IO.Compression.ZipFile]::CreateFromDirectory('${dir}', '${zip}')`,
    ],
    { encoding: 'utf8' },
  )
  assert.equal(res.status, 0, `${res.stdout}${res.stderr}`)
}

// ------------------------------------------------------------------ the registry

/**
 * Builds `version`'s Windows archive for each target, then serves the
 * registry's script for `requested` (a tag or the version) at
 * /registry/install/iii/<requested>.ps1, as GET /install/iii/<requested>.ps1
 * would. `broken`: the archives hold an iii that does not run. `served`: the
 * version the script says it installs, when the test needs the registry to
 * answer with another one.
 */
function publish(version, requested, { targets = [WINDOWS_TARGET], broken = false, digest, pinned } = {}) {
  let preamble = PREAMBLE.replaceAll('https://cdn.iii.cloud/workers/iii/@VERSION@/', `${base}/cdn/${version}/`)
    .replaceAll('https://iii.dev/registry', `${base}/registry`)
    .replaceAll('@VERSION@', pinned ?? version)
    .replaceAll('@REQUESTED@', requested)
  for (const target of targets) {
    const stage = join(ROOT, 'stage', `${version}-${broken ? 'broken' : 'ok'}`, target)
    rmSync(stage, { recursive: true, force: true })
    fakeIii(version, join(stage, 'iii.exe'), { broken })
    const zip = join(ROOT, 'www', 'cdn', version, `iii-${target}.zip`)
    mkdirSync(dirname(zip), { recursive: true })
    zipDirectory(stage, zip)
    const sum = digest ?? createHash('sha256').update(readFileSync(zip)).digest('hex')
    preamble = preamble.replaceAll(`@sha256:${target}@`, sum)
  }
  const script = `${preamble}\n${BODY.endsWith('\n') ? BODY : `${BODY}\n`}}\n`
  const file = join(ROOT, 'www', 'registry', 'install', 'iii', `${requested}.ps1`)
  mkdirSync(dirname(file), { recursive: true })
  writeFileSync(file, script)
  return file
}

/** A clean home, bin dir and event log for one test. */
function sandbox(name) {
  const dir = join(ROOT, 'sandboxes', name)
  rmSync(dir, { recursive: true, force: true })
  mkdirSync(join(dir, 'home'), { recursive: true })
  return { dir, bin: join(dir, 'bin'), log: join(dir, 'events.log') }
}

/**
 * What `irm <base>/registry/install/iii/<requested>.ps1 | iex` runs, with the
 * sandbox's environment. Asynchronous: the server above is in this process.
 */
async function install(shell, box, requested, env = {}) {
  const url = `${base}/registry/install/iii/${requested}.ps1`
  const command = `$ProgressPreference = 'SilentlyContinue'; irm '${url}' -UseBasicParsing | iex; Write-Output ('leaked=' + [bool](Get-Variable state -ErrorAction SilentlyContinue))`
  const childEnv = {
    ...process.env,
    HOME: join(box.dir, 'home'),
    USERPROFILE: join(box.dir, 'home'),
    BIN_DIR: box.bin,
    III_FAKE_LOG: box.log,
    III_TELEMETRY_ENABLED: 'false',
    NO_MODIFY_PATH: '1',
    PATH: hermeticPath(shell),
    ...(ON_WINDOWS ? {} : { TARGET: WINDOWS_TARGET }),
    ...env,
  }
  for (const key of ['VERSION', 'III_RELEASE_TAG', 'III_INSTALL_REDIRECTED', 'III_NON_INTERACTIVE', 'PREFIX']) {
    if (!(key in env)) delete childEnv[key]
  }
  for (const [key, value] of Object.entries(env)) if (value === undefined) delete childEnv[key]
  const child = spawn(shell, ['-NoProfile', '-NonInteractive', '-ExecutionPolicy', 'Bypass', '-Command', command], {
    env: childEnv,
  })
  let stdout = ''
  let stderr = ''
  child.stdout.on('data', (d) => {
    stdout += d
  })
  child.stderr.on('data', (d) => {
    stderr += d
  })
  const status = await new Promise((resolve) => child.on('close', resolve))
  // Windows PowerShell wraps an error at the width of the console: messages
  // are matched in `output`, with the whitespace collapsed.
  return { status, stdout, stderr, output: (stdout + stderr).replace(/\s+/g, ' ') }
}

/** The telemetry events logged so far, waiting up to 10 s for `count` of them (some are sent in the background). */
async function events(box, count) {
  for (let i = 0; i < 100; i++) {
    const lines = existsSync(box.log) ? readFileSync(box.log, 'utf8').split('\n').filter(Boolean) : []
    if (lines.length >= count) {
      return lines.map((line) => {
        const [by, ...args] = line.split('|')
        assert.equal(args[0], '--install-only-generate-ids')
        assert.equal(args[1], '--install-event-type')
        assert.equal(args[3], '--install-event-properties')
        return { by, type: args[2], properties: JSON.parse(args.slice(4).join('|')) }
      })
    }
    await new Promise((resolve) => setTimeout(resolve, 100))
  }
  assert.fail(`expected ${count} telemetry event(s) in ${box.log}`)
}

function installedVersion(box) {
  const exe = join(box.bin, 'iii.exe')
  if (!existsSync(exe)) return null
  return spawnSync(exe, ['--version'], { encoding: 'utf8' }).stdout.trim()
}

// ------------------------------------------------------------------ tests

// CI runs this file to test those shells: it must not pass by finding none.
it('finds the PowerShells this machine should have', { skip: !process.env.CI }, () => {
  const names = SHELLS.map((shell) => shell.split(/[\\/]/).pop().toLowerCase())
  assert.deepEqual(names, ON_WINDOWS ? ['pwsh.exe', 'powershell.exe'] : ['pwsh'])
})

describe('the body alone', () => {
  for (const shell of SHELLS) {
    it(`${shell}: points at the registry and installs nothing`, async () => {
      const box = sandbox(`alone-${shell}`)
      const file = join(ROOT, 'www', 'registry', 'install', 'iii', 'body-only.ps1')
      mkdirSync(dirname(file), { recursive: true })
      writeFileSync(file, `& {\n${BODY}\n}\n`)
      const res = await install(shell, box, 'body-only')
      assert.notEqual(res.status, 0)
      assert.match(res.output, /run it from the registry: irm https:\/\/iii\.dev\/registry\/install\/iii\/latest\.ps1 \| iex/)
      assert.equal(existsSync(join(box.bin, 'iii.exe')), false)
    })
  }
})

for (const shell of SHELLS) {
  describe(`install.ps1 under ${shell}`, { timeout: 120_000 }, () => {
    it('parses, composed as the registry serves it', () => {
      const file = publish('0.27.0', 'parse')
      const res = spawnSync(
        shell,
        [
          '-NoProfile',
          '-NonInteractive',
          '-Command',
          `$errors = $null; $null = [System.Management.Automation.Language.Parser]::ParseFile('${file}', [ref]$null, [ref]$errors); $errors | ForEach-Object { $_.Message }; exit $errors.Count`,
        ],
        { encoding: 'utf8' },
      )
      assert.equal(res.status, 0, res.stdout + res.stderr)
    })

    it('installs a fresh iii, reports it, and prints the setup command', async () => {
      const box = sandbox(`fresh-${shell}`)
      publish('0.27.0', 'latest')
      const res = await install(shell, box, 'latest')
      assert.equal(res.status, 0, res.output)
      assert.match(res.stdout, /installing iii v0\.27\.0 \(latest\)/)
      assert.match(res.stdout, /downloading iii-x86_64-pc-windows-msvc\.zip\.\.\./)
      assert.ok(res.stdout.includes(`installed iii v0.27.0 to ${join(box.bin, 'iii.exe')}`), res.stdout)
      assert.ok(res.stdout.includes(`add ${box.bin} to your Path to run iii from any terminal`), res.stdout)
      assert.match(res.stdout, /To start the iii harness and see what iii can do, run:\s+iii project init --learn-iii/)
      assert.match(res.stdout, /leaked=False/)
      assert.equal(installedVersion(box), 'iii 0.27.0')

      const sent = await events(box, 2)
      const started = sent.find((e) => e.type === 'install_started')
      assert.equal(started.properties.install_method, 'powershell')
      assert.equal(started.properties.target_binary, 'iii')
      assert.equal(started.properties.os, 'windows')
      const succeeded = sent.find((e) => e.type === 'install_succeeded')
      assert.deepEqual(succeeded.properties, {
        installed_version: '0.27.0',
        install_method: 'powershell',
        target_binary: 'iii',
      })
    })

    it('upgrades an older iii in place, with no setup offer', async () => {
      const box = sandbox(`upgrade-${shell}`)
      fakeIii('0.26.0', join(box.bin, 'iii.exe'))
      publish('0.27.0', 'latest')
      const res = await install(shell, box, 'latest')
      assert.equal(res.status, 0, res.output)
      assert.match(res.stdout, /upgraded iii: v0\.26\.0 -> v0\.27\.0/)
      assert.doesNotMatch(res.stdout, /iii harness/)
      assert.equal(installedVersion(box), 'iii 0.27.0')
      const sent = await events(box, 2)
      assert.deepEqual(sent.find((e) => e.type === 'upgrade_started').properties, {
        from_version: '0.26.0',
        to_version: '0.27.0',
        install_method: 'powershell',
        target_binary: 'iii',
      })
      assert.equal(sent.find((e) => e.type === 'upgrade_succeeded').by, '0.27.0')
    })

    it('does nothing when iii is already at that version', async () => {
      const box = sandbox(`same-${shell}`)
      fakeIii('0.27.0', join(box.bin, 'iii.exe'))
      publish('0.27.0', 'latest')
      served.length = 0
      const res = await install(shell, box, 'latest')
      assert.equal(res.status, 0, res.output)
      assert.match(res.stdout, /iii is already at v0\.27\.0 - nothing to do/)
      assert.deepEqual(
        served.filter((u) => u.startsWith('/cdn/')),
        [],
      )
    })

    it('installs the version VERSION names, through the registry script for it', async () => {
      const box = sandbox(`version-${shell}`)
      publish('0.27.0', 'latest')
      publish('0.26.0', '0.26.0')
      const res = await install(shell, box, 'latest', { VERSION: 'iii/v0.26.0' })
      assert.equal(res.status, 0, res.output)
      assert.ok(res.stdout.includes(`installing iii 0.26.0 from ${base}/registry`), res.stdout)
      assert.match(res.stdout, /installing iii v0\.26\.0\r?\n/)
      assert.equal(installedVersion(box), 'iii 0.26.0')
    })

    it('stops when the registry answers a redirect with another version', async () => {
      const box = sandbox(`loop-${shell}`)
      publish('0.27.0', 'latest')
      publish('0.27.0', '0.25.1')
      const res = await install(shell, box, 'latest', { VERSION: '0.25.1' })
      assert.notEqual(res.status, 0)
      assert.match(res.output, /the registry served iii 0\.27\.0 for 0\.25\.1/)
      assert.equal(existsSync(join(box.bin, 'iii.exe')), false)
    })

    it('installs nothing when the archive does not match its digest', async () => {
      const box = sandbox(`digest-${shell}`)
      publish('0.27.0', 'latest', { digest: '0'.repeat(64) })
      const res = await install(shell, box, 'latest')
      assert.notEqual(res.status, 0)
      assert.match(res.output, /failed to download or verify iii-x86_64-pc-windows-msvc\.zip: checksum mismatch/)
      assert.equal(existsSync(join(box.bin, 'iii.exe')), false)
    })

    // The target detected on a Windows machine: off Windows there is none.
    it('keeps a working iii when the new build does not run here, and reports it', { skip: !ON_WINDOWS }, async () => {
      const box = sandbox(`broken-${shell}`)
      fakeIii('0.26.0', join(box.bin, 'iii.exe'))
      publish('0.27.0', 'latest', { broken: true })
      const res = await install(shell, box, 'latest')
      assert.notEqual(res.status, 0)
      const dest = join(box.bin, 'iii.exe')
      assert.ok(res.output.includes('does not run on this system;'), res.output)
      assert.equal(installedVersion(box), 'iii 0.26.0')
      const [failed] = await events(box, 1)
      assert.equal(failed.type, 'upgrade_failed')
      assert.equal(failed.properties.error_stage, 'verify')
      // The message carries a Windows path: its backslashes survive the command line.
      assert.ok(failed.properties.error_message.includes(`${dest} v0.26.0 is unchanged`), failed.properties.error_message)
    })

    it('installs a build that does not run here when TARGET names it, with a warning', async () => {
      const box = sandbox(`target-${shell}`)
      publish('0.27.0', 'latest', { broken: true })
      const res = await install(shell, box, 'latest', { TARGET: WINDOWS_TARGET })
      assert.equal(res.status, 0, res.output)
      assert.ok(res.stdout.includes(`installed iii v0.27.0 to ${join(box.bin, 'iii.exe')}`), res.stdout)
      assert.match(res.output, /the x86_64-pc-windows-msvc build may not run on this system/)
    })

    it('points macOS and Linux at the sh installer', { skip: ON_WINDOWS }, async () => {
      const box = sandbox(`unix-${shell}`)
      publish('0.27.0', 'latest')
      const res = await install(shell, box, 'latest', { TARGET: undefined })
      assert.notEqual(res.status, 0)
      assert.ok(
        res.output.includes(
          `this is the Windows installer; on macOS and Linux run: curl -fsSL ${base}/registry/install/iii/latest.sh | sh`,
        ),
        res.output,
      )
    })

    it('names the builds there are when this version has none for the target', async () => {
      const box = sandbox(`missing-${shell}`)
      publish('0.27.0', 'latest')
      const res = await install(shell, box, 'latest', { TARGET: 'riscv64gc-unknown-linux-gnu' })
      assert.notEqual(res.status, 0)
      assert.match(res.output, /iii v0\.27\.0 does not include a prebuilt binary for riscv64gc-unknown-linux-gnu\./)
      assert.match(res.stderr, /Available iii targets in v0\.27\.0:(\r?\n  - [a-z0-9_-]+)*\r?\n  - x86_64-pc-windows-msvc\r?\n/)
      assert.match(res.output, /no release asset found for target: riscv64gc-unknown-linux-gnu/)
    })
  })
}
