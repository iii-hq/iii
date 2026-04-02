import { enqueue, type Handlers, jsonSchema, logger, type StepConfig } from 'motia'
import { z } from 'zod'
import { petStoreService } from '../basic-tutorial/services/pet-store'
import { petSchema } from '../basic-tutorial/services/types'

export const config = {
  name: 'ArrayStep',
  description: 'Basic API Example step with Array in Body and in Response',
  flows: ['array-step'],
  triggers: [
    {
      type: 'http',
      method: 'POST',
      path: '/array',
      bodySchema: jsonSchema(
        z.array(
          z.object({
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
        ),
      ),
      responseSchema: {
        200: jsonSchema(z.array(petSchema)),
      },
    },
  ],
  enqueues: ['process-food-order'],
} as const satisfies StepConfig

export const handler: Handlers<typeof config> = async ({ request }) => {
  logger.info('Step 01 - Processing API Step', { body: request.body })

  const [{ pet, foodOrder }] = request.body || [{}]
  const newPetRecord = await petStoreService.createPet(pet)

  logger.info('Pet and food order', { pet, foodOrder })

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

  return { status: 200, body: [newPetRecord] }
}
