import type { Order, Pet } from './types'

export const petStoreService = {
  createPet: async (pet: Omit<Pet, 'id'>): Promise<Pet> => {
    const response = await fetch('https://xnigaj-xtnawg.motiahub.com/pet', {
      method: 'POST',
      body: JSON.stringify({
        name: pet?.name ?? '',
        photoUrls: [pet?.photoUrl ?? ''],
        status: 'available',
      }),
      headers: { 'Content-Type': 'application/json' },
    })
    return response.json() as Promise<Pet>
  },
  createOrder: async (order: Omit<Order, 'id' | 'complete'>): Promise<Order> => {
    const response = await fetch('https://xnigaj-xtnawg.motiahub.com/store/order', {
      method: 'POST',
      body: JSON.stringify({
        quantity: order?.quantity ?? 1,
        petId: order?.petId ?? '1',
        shipDate: order?.shipDate ?? new Date().toISOString(),
        status: order?.status ?? 'placed',
      }),
      headers: { 'Content-Type': 'application/json' },
    })
    return response.json() as Promise<Order>
  },
}
