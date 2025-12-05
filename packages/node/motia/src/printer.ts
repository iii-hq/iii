import path from 'path'
import pc from 'picocolors'
import { isApiStep, isCronStep, isEventStep, isNoopStep } from './guards'
import type { ValidationError } from './step-validator'
import type { Step } from './types'
import type { Stream } from './types-stream'

const stepTag = pc.bold(pc.magenta('Step'))
const flowTag = pc.bold(pc.blue('Flow'))
const streamTag = pc.bold(pc.green('Stream'))
const registered = pc.green('➜ [REGISTERED]')
const building = pc.yellow('⚡ [BUILDING]')
const built = pc.green('✓ [BUILT]')
const updated = pc.yellow('➜ [UPDATED]')
const removed = pc.red('➜ [REMOVED]')
const invalidEmit = pc.red('➜ [INVALID EMIT]')
const error = pc.red('[ERROR]')
const warning = pc.yellow('[WARNING]')
const warnIcon = pc.yellow('⚠')
const infoIcon = pc.blue('ℹ')
const errorIcon = pc.red('✖')

export class Printer {
  constructor(private readonly baseDir: string) {}

  stepTag = stepTag
  flowTag = flowTag
  registered = registered
  building = building
  built = built
  updated = updated
  removed = removed

  printEventInputValidationError(
    emit: { topic: string },
    details: { missingFields?: string[]; extraFields?: string[]; typeMismatches?: string[] },
  ) {
    const emitPath = pc.bold(pc.cyan(`Emit ${emit.topic}`))

    console.log(`${warnIcon} ${warning} ${emitPath} validation issues:`)

    const hasAny = details.missingFields?.length || details.extraFields?.length || details.typeMismatches?.length

    if (!hasAny) {
      console.log(`${pc.yellow('│')} No issues found.`)
      console.log(`${pc.yellow('└─')} Validation passed.`)
      return
    }

    if (details.missingFields?.length) {
      console.log(`${pc.yellow('│')} ${pc.yellow(`⚠ Missing fields: ${details.missingFields.join(', ')}`)}`)
    }

    if (details.extraFields?.length) {
      console.log(`${pc.yellow('│')} ${pc.yellow(`⚠ Extra fields: ${details.extraFields.join(', ')}`)}`)
    }

    if (details.typeMismatches?.length) {
      console.log(`${pc.yellow('│')} ${pc.yellow(`⚠ Type mismatches: ${details.typeMismatches.join(', ')}`)}`)
    }

    console.log(`${pc.yellow('└─')} ${pc.yellow('Payload does not match schema.')}`)
  }

  printInvalidEmit(step: Step, emit: string) {
    console.log(
      `${invalidEmit} ${stepTag} ${this.getStepType(step)} ${this.getStepPath(
        step,
      )} tried to emit an event not defined in the step config: ${pc.yellow(emit)}`,
    )
  }

  printStepCreated(step: Step) {
    console.log(`${registered} ${stepTag} ${this.getStepType(step)} ${this.getStepPath(step)} registered`)
  }

  printStepUpdated(step: Step) {
    console.log(`${updated} ${stepTag} ${this.getStepType(step)} ${this.getStepPath(step)} updated`)
  }

  printStepRemoved(step: Step) {
    console.log(`${removed} ${stepTag} ${this.getStepType(step)} ${this.getStepPath(step)} removed`)
  }

  printFlowCreated(flowName: string) {
    console.log(`${registered} ${flowTag} ${pc.bold(pc.cyan(flowName))} registered`)
  }

  printFlowUpdated(flowName: string) {
    console.log(`${updated} ${flowTag} ${pc.bold(pc.cyan(flowName))} updated`)
  }

  printFlowRemoved(flowName: string) {
    console.log(`${removed} ${flowTag} ${pc.bold(pc.cyan(flowName))} removed`)
  }

  printStreamCreated(stream: Stream) {
    console.log(`${registered} ${streamTag} ${this.getStreamPath(stream)} registered`)
  }

  printStreamUpdated(stream: Stream) {
    console.log(`${updated} ${streamTag} ${this.getStreamPath(stream)} updated`)
  }

  printStreamRemoved(stream: Stream) {
    console.log(`${removed} ${streamTag} ${this.getStreamPath(stream)} removed`)
  }

  printInvalidEmitConfiguration(step: Step, emit: string) {
    console.log(
      `${warnIcon} ${warning} ${stepTag} ${this.getStepType(step)} ${this.getStepPath(step)} emits to ${pc.yellow(
        emit,
      )}, but there is no subscriber defined`,
    )
  }

  printInvalidSchema(topic: string, step: Step[]) {
    console.log(`${error} Topic ${pc.bold(pc.blue(topic))} has incompatible schemas in the following steps:`)
    step.forEach((step) => {
      console.log(`${pc.red('  ✖')} ${this.getStepPath(step)}`)
    })
  }

  printValidationError(stepPath: string, validationError: ValidationError) {
    const relativePath = this.getRelativePath(stepPath)

    console.log(`${error} ${pc.bold(pc.cyan(relativePath))}`)
    validationError.errors?.forEach((error) => {
      if (error.path) {
        console.log(`${pc.red('│')} ${pc.yellow(`✖ ${error.path}`)}: ${error.message}`)
      } else {
        console.log(`${pc.red('│')} ${pc.yellow('✖')} ${error.message}`)
      }
    })
    console.log(`${pc.red('└─')} ${pc.red(validationError.error)}  `)
  }

  getRelativePath(filePath: string) {
    return path.relative(this.baseDir, filePath)
  }

  getStepType(step: Step) {
    if (isApiStep(step)) return pc.gray('(API)')
    if (isEventStep(step)) return pc.gray('(Event)')
    if (isCronStep(step)) return pc.gray('(Cron)')
    if (isNoopStep(step)) return pc.gray('(Noop)')

    return pc.gray('(Unknown)')
  }

  getStepPath(step: Step) {
    const stepPath = this.getRelativePath(step.filePath)
    return pc.bold(pc.cyan(stepPath))
  }

  getStreamPath(stream: Stream) {
    const streamPath = this.getRelativePath(stream.filePath)
    return pc.bold(pc.magenta(streamPath))
  }

  printPluginLog(message: string) {
    const pluginTag = pc.bold(pc.cyan('[motia-plugins]'))
    console.log(`${infoIcon} ${pluginTag} ${message}`)
  }

  printPluginWarn(message: string) {
    const pluginTag = pc.bold(pc.cyan('[motia-plugins]'))
    console.warn(`${warnIcon} ${pluginTag} ${pc.yellow(message)}`)
  }

  printPluginError(message: string, ...args: unknown[]) {
    const pluginTag = pc.bold(pc.cyan('[motia-plugins]'))
    console.error(`${errorIcon} ${pluginTag} ${pc.red(message)}`, ...args)
  }
}

export class NoPrinter extends Printer {
  constructor() {
    super('')
  }

  printEventInputValidationError() {}

  printInvalidEmit() {}
  printStepCreated() {}
  printStepUpdated() {}
  printStepRemoved() {}
  printFlowCreated() {}
  printFlowUpdated() {}
  printFlowRemoved() {}
  printStepType() {}
  printStepPath() {}

  printStreamCreated() {}
  printStreamUpdated() {}
  printStreamRemoved() {}

  printPluginLog() {}
  printPluginWarn() {}
  printPluginError() {}
}
