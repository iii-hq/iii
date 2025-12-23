import { AuthenticateStream } from '@iii-dev/motia'

export const authenticateStream: AuthenticateStream = async (req, context) => {
  context.logger.info('Authenticating stream', { req })

  return {
    context: { userId: 'sergio' },
  }
}
