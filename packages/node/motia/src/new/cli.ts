import { Command } from 'commander'
import { dev } from './build/dev'
import { build } from './build/build'

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

program.parse(process.argv)
