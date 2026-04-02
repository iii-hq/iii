import { execSync } from 'child_process'
import { existsSync } from 'fs'
import { mkdir, rm, writeFile } from 'fs/promises'
import { join } from 'path'
import { createInterface } from 'readline'

const REPO = 'MotiaDev/motia-iii-example'
const BRANCH = 'main'
const TEMPLATE_PREFIX = 'nodejs'

const BLUE = '\x1b[1;34m'
const LIGHT_BLUE = '\x1b[94m'
const R = '\x1b[0m'
const WHITE = '\x1b[97m'
const GRAY = '\x1b[90m'
const YELLOW = '\x1b[33m'
const LIGHT_YELLOW = '\x1b[93m'

const BANNER = `
  ${LIGHT_BLUE}╭───────────────────────────────────────╮${R}
  ${LIGHT_BLUE}│${R}  ${LIGHT_YELLOW}==${R} Welcome to ${BLUE}Motia${R} powered by iii   ${LIGHT_BLUE}│${R}
  ${LIGHT_BLUE}╰───────────────────────────────────────╯${R}

${LIGHT_BLUE}░${BLUE}███     ░███               ░██    ░██                ${LIGHT_YELLOW}░${YELLOW}████████████         
${LIGHT_BLUE}░${BLUE}████   ░████               ░██                      ${LIGHT_YELLOW}░${YELLOW}██         ░██              
${LIGHT_BLUE}░${BLUE}██░██ ░██░██  ░███████  ░████████ ░██ ░██████      ${LIGHT_YELLOW}░${YELLOW}██  ░██████  ░██    ${GRAY}░${WHITE}██${GRAY}░${WHITE}██${GRAY}░${WHITE}██
${LIGHT_BLUE}░${BLUE}██ ░████ ░██ ░██    ░██    ░██    ░██      ░██     ${LIGHT_YELLOW}░${YELLOW}██       ░██ ░██    
${LIGHT_BLUE}░${BLUE}██  ░██  ░██ ░██    ░██    ░██    ░██ ░███████     ${LIGHT_YELLOW}░${YELLOW}██  ░███████ ░██    ${GRAY}░${WHITE}██${GRAY}░${WHITE}██${GRAY}░${WHITE}██
${LIGHT_BLUE}░${BLUE}██       ░██ ░██    ░██    ░██    ░██░██   ░██     ${LIGHT_YELLOW}░${YELLOW}██ ░██   ░██ ░██    ${GRAY}░${WHITE}██${GRAY}░${WHITE}██${GRAY}░${WHITE}██
${LIGHT_BLUE}░${BLUE}██       ░██  ░███████      ░████ ░██ ░█████░██    ${LIGHT_YELLOW}░${YELLOW}██  ░█████░████     ${GRAY}░${WHITE}██${GRAY}░${WHITE}██${GRAY}░${WHITE}██
                                                     ${LIGHT_YELLOW}░${YELLOW}██                          
                                                      ${LIGHT_YELLOW}░${YELLOW}████████████               

  ${LIGHT_YELLOW}-${LIGHT_BLUE} Create a new Motia project powered by iii${R}
`

const SKIP_FILES = new Set(['package-lock.json', 'README.md'])
const BOLD = '\x1b[1m'
const RED = '\x1b[31m'
const FETCH_TIMEOUT_MS = 30_000

interface RepoTreeEntry {
  path: string
  type: string
}

function ask(rl: ReturnType<typeof createInterface>, question: string): Promise<string> {
  return new Promise((resolve) => {
    rl.question(question, (answer) => resolve(answer.trim()))
  })
}

function fetchHeaders(): Record<string, string> {
  const headers: Record<string, string> = { 'User-Agent': 'motia-cli' }
  const token = process.env.GITHUB_TOKEN
  if (token) {
    headers.Authorization = `Bearer ${token}`
  }
  return headers
}

async function fetchRepoTree(): Promise<RepoTreeEntry[]> {
  const url = `https://api.github.com/repos/${REPO}/git/trees/${BRANCH}?recursive=1`
  const res = await fetch(url, {
    headers: fetchHeaders(),
    signal: AbortSignal.timeout(FETCH_TIMEOUT_MS),
  })

  if (res.status === 403 || res.status === 429) {
    const resetHeader = res.headers.get('x-ratelimit-reset')
    const resetMsg = resetHeader
      ? ` Rate limit resets at ${new Date(Number(resetHeader) * 1000).toLocaleTimeString()}.`
      : ''
    throw new Error(`GitHub API rate limit exceeded.${resetMsg} Set GITHUB_TOKEN to increase your limit.`)
  }

  if (!res.ok) {
    throw new Error(`Failed to fetch template repository: ${res.statusText}`)
  }

  const data = (await res.json()) as { tree: RepoTreeEntry[] }
  const prefix = `${TEMPLATE_PREFIX}/`
  return data.tree.filter(
    (entry) =>
      entry.type === 'blob' && entry.path.startsWith(prefix) && !SKIP_FILES.has(entry.path.split('/').pop() ?? ''),
  )
}

async function downloadFile(filePath: string): Promise<string> {
  const url = `https://raw.githubusercontent.com/${REPO}/${BRANCH}/${filePath}`
  const res = await fetch(url, {
    headers: fetchHeaders(),
    signal: AbortSignal.timeout(FETCH_TIMEOUT_MS),
  })

  if (!res.ok) {
    throw new Error(`Failed to download ${filePath}: ${res.statusText}`)
  }

  return res.text()
}

export async function create() {
  console.log(BANNER)

  const rl = createInterface({ input: process.stdin, output: process.stdout })

  try {
    let folderName = ''
    let targetDir = ''
    let emptyCount = 0

    while (true) {
      folderName = await ask(rl, '  Project folder name: ')

      if (!folderName) {
        emptyCount++
        if (emptyCount >= 2) {
          console.log('\n  Project creation cancelled.\n')
          return
        }
        console.error('\n  Project folder name is required. Press Enter again to cancel.\n')
        continue
      }
      emptyCount = 0

      targetDir = join(process.cwd(), folderName)

      if (existsSync(targetDir)) {
        console.error(`\n  Directory "${folderName}" already exists. Please choose a different name.\n`)
        continue
      }

      break
    }

    const hasIII = await ask(rl, '  Do you have iii installed? (Y/n): ')

    if (hasIII.toLowerCase() === 'n' || hasIII.toLowerCase() === 'no') {
      console.log('')
      console.log('  Motia is now powered by iii for step orchestration.')
      console.log('  iii is the backend engine that runs your Motia steps,')
      console.log('  handling APIs, queues, state, and workflows in a single runtime.')
      console.log('')
      console.log(`  Install iii → ${BOLD}https://iii.dev/docs${R}`)
      console.log('')

      const cont = await ask(rl, '  Continue creating project? (Y/n): ')
      if (cont.toLowerCase() === 'n' || cont.toLowerCase() === 'no') {
        console.log('\n  Project creation cancelled.\n')
        return
      }
    }

    console.log('')
    console.log(`  Creating project in ./${folderName}`)
    console.log('')

    let files: RepoTreeEntry[]
    try {
      files = await fetchRepoTree()
    } catch (err: unknown) {
      const name = err instanceof Error ? err.name : ''
      const msg = err instanceof Error ? err.message : String(err)
      if (name === 'TimeoutError' || name === 'AbortError') {
        console.error(`\n  ${RED}Connection timed out.${R} Check your internet connection and try again.\n`)
      } else if (msg.includes('rate limit')) {
        console.error(`\n  ${RED}${msg}${R}\n`)
      } else {
        console.error(`\n  ${RED}Failed to fetch project template:${R} ${msg}\n`)
      }
      process.exitCode = 1
      return
    }

    const prefix = `${TEMPLATE_PREFIX}/`
    const dirs = new Set<string>()
    for (const file of files) {
      const relPath = file.path.startsWith(prefix) ? file.path.slice(prefix.length) : file.path
      const lastSlash = relPath.lastIndexOf('/')
      if (lastSlash > 0) dirs.add(relPath.substring(0, lastSlash))
    }

    await mkdir(targetDir, { recursive: true })

    for (const dir of dirs) {
      await mkdir(join(targetDir, dir), { recursive: true })
    }

    try {
      for (const file of files) {
        const relPath = file.path.startsWith(prefix) ? file.path.slice(prefix.length) : file.path
        process.stdout.write(`  ↓ ${relPath}\n`)
        let content = await downloadFile(file.path)

        if (relPath === 'package.json') {
          const pkg = JSON.parse(content)
          pkg.name = folderName
          content = JSON.stringify(pkg, null, 2) + '\n'
        }

        await writeFile(join(targetDir, relPath), content)
      }
    } catch (err: unknown) {
      const name = err instanceof Error ? err.name : ''
      const msg = err instanceof Error ? err.message : String(err)
      if (name === 'TimeoutError' || name === 'AbortError') {
        console.error(`\n  ${RED}Download timed out.${R} Check your internet connection and try again.\n`)
      } else {
        console.error(`\n  ${RED}Failed to download files:${R} ${msg}\n`)
      }
      try {
        await rm(targetDir, { recursive: true, force: true })
        console.error(`  Cleaned up partial directory ./${folderName}\n`)
      } catch {
        // cleanup is best-effort; ignore errors
      }
      process.exitCode = 1
      return
    }

    console.log('')
    console.log('  Installing dependencies...')
    console.log('')

    try {
      execSync('npm install', { cwd: targetDir, stdio: 'inherit' })
    } catch {
      console.error(`\n  ${RED}Failed to install dependencies.${R} Run "npm install" manually in ./${folderName}\n`)
      process.exitCode = 1
      return
    }

    console.log('')
    console.log('  ✓ Project created successfully!')
    console.log('')
    console.log('  Next steps:')
    console.log(`    cd ${folderName}`)
    console.log('    iii -c iii-config.yaml')
    console.log('')
  } finally {
    rl.close()
  }
}
