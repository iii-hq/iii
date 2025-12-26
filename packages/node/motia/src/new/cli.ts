import { Command } from 'commander'
import { dev } from './build/dev'
import { build } from './build/build'
import { typegen } from './build/typegen'

const program = new Command()

program
  .command('dev')
  .description('Build the project for development')
  .action(() => {
    dev().catch(console.error)
  })

program
  .command('build')
  .description('Build the project for production')
  .option('-e, --external <external>', 'External dependencies')
  .action((options) => {
    const external = options.external ? options.external.split(',') : []

    build({ external }).catch(console.error)
  })

program
  .command('typegen')
  .description('Generate TypeScript types from steps and streams')
  .option('-w, --watch', 'Watch for file changes')
  .option('-o, --output <path>', 'Output file path', 'types.d.ts')
  .action((options) => {
    typegen(options).catch(console.error)
  })

program.parse(process.argv)
