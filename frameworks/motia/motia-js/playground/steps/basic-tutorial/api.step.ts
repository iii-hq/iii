import { enqueue, type Handlers, type StepConfig } from 'motia'
import { z } from 'zod'
import { petStoreService } from './services/pet-store'
import { petSchema } from './services/types'

export const config = {
  name: 'ApiTrigger',
  description: 'basic-tutorial api trigger',
  flows: ['basic-tutorial'],
  triggers: [
    {
      type: 'http',
      method: 'POST',
      path: '/basic-tutorial',
      bodySchema: z.object({
        pet: z.object({
          name: z.string(),
          photoUrl: z.string(),
        }),
        foodOrder: z
          .object({
            quantity: z.number(),
          })
          .optional(),
      }),
      responseSchema: {
        200: petSchema,
      },
    },
  ],
  enqueues: ['process-food-order'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async ({ request }, { logger, traceId }) => {
  logger.info('Step 01 - Processing API Step', { body: request.body })

  const { pet, foodOrder } = request.body || {}
  const newPetRecord = await petStoreService.createPet(pet)

  if (foodOrder) {
    await enqueue({
      topic: 'process-food-order',
      data: {
        quantity: foodOrder.quantity,
        email: 'test@test.com',
        petId: newPetRecord.id,
      },
    })
  }

  return { status: 200, body: { ...newPetRecord, traceId } }
}
