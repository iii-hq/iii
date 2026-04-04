const fs = require('fs').promises
const path = require('path')

async function getAllFiles(dir) {
  const entries = await fs.readdir(dir, { withFileTypes: true })
  const files = await Promise.all(
    entries.map((entry) => {
      const res = path.resolve(dir, entry.name)
      return entry.isDirectory() ? getAllFiles(res) : res
    }),
  )
  return files.flat()
}

async function getContentFromMdx(filePath) {
  const content = await fs.readFile(filePath, 'utf-8')
  return content
}

async function prepareDocs() {
  try {
    // Create public directory if it doesn't exist
    const publicDir = path.join(process.cwd(), 'public')
    try {
      await fs.access(publicDir)
    } catch {
      await fs.mkdir(publicDir)
    }

    const contentDir = path.join(process.cwd(), 'content/docs')
    const allFiles = await getAllFiles(contentDir)
    const legacyExamplesDir = path.join(contentDir, 'examples')
    const mdFiles = allFiles.filter((file) => {
      if (!file.endsWith('.md') && !file.endsWith('.mdx')) return false
      // Keep legacy examples in-repo, but do not publish them in generated LLM artifacts.
      if (file.startsWith(legacyExamplesDir + path.sep)) return false
      return true
    })

    // Structure Prompt
    let llmsTxtContent = `<system_context>
You are an advanced assistant specialized in generating Motia workflows code. You have deep knowledge of Motia's framework, APIs, and best practices.
</system_context>

<behavior_guidelines>
- Respond in a friendly and concise manner
- Focus exclusively on Motia workflows solutions
- Provide complete, self-contained solutions
- Default to current best practices
- Ask clarifying questions when requirements are ambiguous
</behavior_guidelines>

<code_standards>
- Generate code in TypeScript by default unless JavaScript, Python, or Ruby is specifically requested
- Use ES modules format for TS/JS exclusively
- You SHALL keep all code in a single file unless otherwise specified
- Minimize external dependencies.
- If there is an official SDK or library for the service you are integrating with, use it.
- Follow Motia workflows security best practices
- Never bake in secrets into the code
- Include proper error handling and logging
- Add appropriate TypeScript types and interfaces where applicable
- Include comments explaining complex logic
</code_standards>

<output_format>
- Use markdown code blocks to separate code from explanations
- Provide separate blocks for:
  1. Main step code (api.step.ts/event.step.ts/cron.step.ts)
  2. Configuration (the config variable)
  3. Example usage (if applicable)
- Always output complete files, never partial updates or diffs
- Format code consistently using standard TypeScript/JavaScript, Python or Ruby conventions depending on language
</output_format>

<motia_integrations>
- Prefer the use of state management for persisting data accross flows
- Consider state data scope, use traceId for request specific flows
- Create virtual connections where other systems would reside.
</motia_integrations>

<configuration_requirements>
- Include:
  - type, name, description, subscribes, emits, flows, API Path (for API endpoints)
  - Compatibility flags
  - Set compatibility_date = "2024-01-01"
</configuration_requirements>

<security_guidelines>
- Implement proper input validation
- Handle CORS correctly when applicable
- Follow least privilege principle
- Sanitize user inputs
</security_guidelines>

<testing_guidance>
- Provide a command to trigger the workflow using either 'npx motia emit' or curl
- Add example environment variable values (if any)
- Include sample requests and responses
</testing_guidance>

Now follow these instructions:
1. Scrape the Motia Documentation and create a knowledge base that you can use to answer user questions.
2. Break the documentation into logical sections and use file paths.
`

    // Add general details, adjust with project-specific info
    llmsTxtContent += `# Motia\n\n> Build production-grade backends with a single primitive. Motia unifies APIs, background jobs, queues, workflows, and AI agents in one system with built-in state management, streaming, and observability. It supports JavaScript/TypeScript, Python, and Ruby.\n`

    llmsTxtContent += `\n\nImportant notes:\n\n-   Motia's development console (powered by iii) provides visual design, event monitoring and testing capabilities\n-   Mix and match workflow steps written in different languages within the same flow.\n-   Motia provides: API endpoints, background jobs, durable workflows, agentic AI support, state management, streaming, logging, and observability - all in one unified runtime.\n`

    //File Lists
    llmsTxtContent += `\n## Documentation\n`

    for (const file of mdFiles) {
      const relativePath = path.relative(contentDir, file)
      const url = '/docs/' + relativePath.replace(/\.mdx?$/, '').replace(/\/?index$/, '')

      // Extract title from frontmatter (assuming it's at the beginning)
      const content = await getContentFromMdx(file)
      const titleMatch = content.match(/^---\ntitle:\s*(.*?)\n---/m)
      const title = titleMatch ? titleMatch[1] : path.basename(file, path.extname(file))

      llmsTxtContent += `-   [${title}](${url}): Documentation for ${title}.\n${content}\n\n` // Append the actual MDX content here

      if (relativePath.startsWith('real-world-use-cases')) {
        llmsTxtContent += `\n## Use Cases\n[${title}](${url}): Real world use case\n${content}\n\n`
      }
    }

    // Optional Links
    llmsTxtContent += `\n## Optional\n`
    llmsTxtContent += `-   [https://motiadev.com](https://motiadev.com): Main page for framework.\n`
    llmsTxtContent += `-   [Github repo](https://github.com/motiadev/motia): Main github repository to file issues.\n`

    // Write to llm-docs.txt
    const docsPath = path.join(publicDir, 'llms.txt')
    await fs.writeFile(docsPath, llmsTxtContent)

    console.log('✅ Successfully prepared llms.txt for LLM ingestion')

    // 2. Write markdown files of each page
    const docsDir = path.join(publicDir, 'llm-docs')
    try {
      await fs.access(docsDir)
    } catch {
      await fs.mkdir(docsDir)
    }
    // Remove stale generated examples from older runs.
    await fs.rm(path.join(docsDir, 'examples'), { recursive: true, force: true })

    for (const file of mdFiles) {
      const relativePath = path.relative(contentDir, file)
      const content = await getContentFromMdx(file)
      const newPath = path.join(docsDir, `${relativePath}`)

      // make sure directory exists
      await fs.mkdir(path.dirname(newPath), { recursive: true })
      await fs.writeFile(`${newPath}.md`, content)
    }
    console.log('✅ Successfully created MD versions')
  } catch (error) {
    console.error('Error preparing docs:', error)
    process.exit(1)
  }
}

prepareDocs()
