import { z } from 'zod'

export const petSchema = z.object({
  id: z.string(),
  name: z.string(),
  photoUrl: z.string(),
})

export const orderStatusSchema = z.enum(['placed', 'approved', 'delivered'])

export const orderSchema = z.object({
  id: z.string(),
  quantity: z.number(),
  petId: z.string(),
  shipDate: z.string(),
  status: orderStatusSchema,
  complete: z.boolean(),
})

export type Pet = z.infer<typeof petSchema>
export type Order = z.infer<typeof orderSchema>
export type OrderStatus = z.infer<typeof orderStatusSchema>
