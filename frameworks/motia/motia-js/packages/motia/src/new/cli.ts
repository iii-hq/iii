import { Command } from 'commander'
import { build } from './build/build'
import { dev } from './build/dev'
import { create } from './create'

const program = new Command()

program
  .command('dev')
  .description('Build the project for development')
  .action(() => {
    dev().catch((err) => {
      console.error(err)
      process.exitCode = 1
    })
  })

program
  .command('build')
  .description('Build the project for production')
  .option('-e, --external <external>', 'External dependencies')
  .action((options) => {
    const external = options.external ? options.external.split(',') : []

    build({ external }).catch((err) => {
      console.error(err)
      process.exitCode = 1
    })
  })

program
  .command('create')
  .description('Create a new Motia project powered by iii')
  .action(() => {
    create().catch((err) => {
      console.error(err)
      process.exitCode = 1
    })
  })

program.parse(process.argv)
