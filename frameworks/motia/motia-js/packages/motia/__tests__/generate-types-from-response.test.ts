import { generateTypesFromResponse } from '../src/types/generate-types-from-response'

describe('generateTypesFromResponse', () => {
  it('generates ApiResponse union from status-schema map', () => {
    const record = { 200: { type: 'object', properties: { id: { type: 'string' } } } }
    const result = generateTypesFromResponse(record as any)
    expect(result).toContain('ApiResponse<200,')
    expect(result).toContain('id')
  })

  it('handles multiple status codes', () => {
    const record = {
      200: { type: 'string' },
      404: { type: 'object', properties: { error: { type: 'string' } } },
    }
    const result = generateTypesFromResponse(record as any)
    expect(result).toContain('ApiResponse<200, string>')
    expect(result).toContain('ApiResponse<404,')
    expect(result).toContain(' | ')
  })
})
