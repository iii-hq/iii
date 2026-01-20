import { convertSchemaToJson } from '../utils'

describe('convertSchemaToJson', () => {
  describe('TypeScript step schemas (inline definitions)', () => {
    it('should convert a simple object schema', () => {
      const schema = {
        type: 'object',
        properties: {
          name: { type: 'string' },
          age: { type: 'number' },
        },
        required: ['name'],
      }

      const result = convertSchemaToJson(schema)
      expect(result).toEqual({
        name: 'string',
        age: 0,
      })
    })

    it('should convert a schema with string type', () => {
      const schema = { type: 'string' }
      const result = convertSchemaToJson(schema)
      expect(result).toBe('string')
    })

    it('should convert a schema with string enum', () => {
      const schema = { type: 'string', enum: ['option1', 'option2'] }
      const result = convertSchemaToJson(schema)
      expect(result).toBe('option1')
    })

    it('should convert a schema with number type', () => {
      const schema = { type: 'number' }
      const result = convertSchemaToJson(schema)
      expect(result).toBe(0)
    })

    it('should convert a schema with integer type', () => {
      const schema = { type: 'integer' }
      const result = convertSchemaToJson(schema)
      expect(result).toBe(0)
    })

    it('should convert a schema with boolean type', () => {
      const schema = { type: 'boolean' }
      const result = convertSchemaToJson(schema)
      expect(result).toBe(false)
    })

    it('should convert a schema with null type', () => {
      const schema = { type: 'null' }
      const result = convertSchemaToJson(schema)
      expect(result).toBeNull()
    })

    it('should convert a schema with array type', () => {
      const schema = {
        type: 'array',
        items: { type: 'string' },
      }
      const result = convertSchemaToJson(schema)
      expect(result).toEqual(['string'])
    })

    it('should convert a schema with nested objects', () => {
      const schema = {
        type: 'object',
        properties: {
          user: {
            type: 'object',
            properties: {
              name: { type: 'string' },
              email: { type: 'string' },
            },
          },
        },
      }
      const result = convertSchemaToJson(schema)
      expect(result).toEqual({
        user: {
          name: 'string',
          email: 'string',
        },
      })
    })

    it('should convert a schema with array of objects', () => {
      const schema = {
        type: 'array',
        items: {
          type: 'object',
          properties: {
            id: { type: 'integer' },
            name: { type: 'string' },
          },
        },
      }
      const result = convertSchemaToJson(schema)
      expect(result).toEqual([
        {
          id: 0,
          name: 'string',
        },
      ])
    })

    it('should return empty object for undefined schema', () => {
      const result = convertSchemaToJson(undefined)
      expect(result).toEqual({})
    })
  })

  describe('Python step schemas ($defs/$ref)', () => {
    it('should resolve $ref references to $defs', () => {
      const rootSchema = {
        type: 'object',
        properties: {
          pet: {
            $ref: '#/$defs/PetRequest',
          },
        },
        $defs: {
          PetRequest: {
            type: 'object',
            properties: {
              name: { type: 'string' },
              photoUrl: { type: 'string' },
            },
            required: ['name', 'photoUrl'],
          },
        },
      }

      const result = convertSchemaToJson(rootSchema, rootSchema)
      expect(result).toEqual({
        pet: {
          name: 'string',
          photoUrl: 'string',
        },
      })
    })

    it('should handle nested $ref references', () => {
      const rootSchema = {
        type: 'object',
        properties: {
          order: {
            $ref: '#/$defs/Order',
          },
        },
        $defs: {
          Order: {
            type: 'object',
            properties: {
              customer: {
                $ref: '#/$defs/Customer',
              },
              items: {
                type: 'array',
                items: {
                  $ref: '#/$defs/Item',
                },
              },
            },
          },
          Customer: {
            type: 'object',
            properties: {
              name: { type: 'string' },
              email: { type: 'string' },
            },
          },
          Item: {
            type: 'object',
            properties: {
              id: { type: 'integer' },
              quantity: { type: 'integer' },
            },
          },
        },
      }

      const result = convertSchemaToJson(rootSchema, rootSchema)
      expect(result).toEqual({
        order: {
          customer: {
            name: 'string',
            email: 'string',
          },
          items: [
            {
              id: 0,
              quantity: 0,
            },
          ],
        },
      })
    })

    it('should handle anyOf with nullable fields (Python optional)', () => {
      const rootSchema = {
        type: 'object',
        properties: {
          pet: {
            $ref: '#/$defs/PetRequest',
          },
          foodOrder: {
            anyOf: [
              {
                $ref: '#/$defs/FoodOrder',
              },
              {
                type: 'null',
              },
            ],
            default: null,
          },
        },
        $defs: {
          PetRequest: {
            type: 'object',
            properties: {
              name: { type: 'string' },
              photoUrl: { type: 'string' },
            },
            required: ['name', 'photoUrl'],
          },
          FoodOrder: {
            type: 'object',
            properties: {
              quantity: { type: 'integer' },
            },
            required: ['quantity'],
          },
        },
      }

      const result = convertSchemaToJson(rootSchema, rootSchema)
      expect(result).toEqual({
        pet: {
          name: 'string',
          photoUrl: 'string',
        },
        foodOrder: {
          quantity: 0,
        },
      })
    })

    it('should handle anyOf with all null types', () => {
      const schema = {
        anyOf: [{ type: 'null' }, { type: 'null' }],
      }
      const result = convertSchemaToJson(schema)
      expect(result).toBeNull()
    })

    it('should handle $ref in array items', () => {
      const rootSchema = {
        type: 'object',
        properties: {
          pets: {
            type: 'array',
            items: {
              $ref: '#/$defs/PetRequest',
            },
          },
        },
        $defs: {
          PetRequest: {
            type: 'object',
            properties: {
              name: { type: 'string' },
              age: { type: 'integer' },
            },
          },
        },
      }

      const result = convertSchemaToJson(rootSchema, rootSchema)
      expect(result).toEqual({
        pets: [
          {
            name: 'string',
            age: 0,
          },
        ],
      })
    })

    it('should handle complex Python schema like def-schema.json', () => {
      const rootSchema = {
        type: 'object',
        properties: {
          pet: {
            $ref: '#/$defs/PetRequest',
          },
          foodOrder: {
            anyOf: [
              {
                $ref: '#/$defs/FoodOrder',
              },
              {
                type: 'null',
              },
            ],
            default: null,
          },
        },
        required: ['pet'],
        $defs: {
          FoodOrder: {
            properties: {
              quantity: {
                title: 'Quantity',
                type: 'integer',
              },
            },
            required: ['quantity'],
            title: 'FoodOrder',
            type: 'object',
          },
          PetRequest: {
            properties: {
              name: {
                title: 'Name',
                type: 'string',
              },
              photoUrl: {
                title: 'Photourl',
                type: 'string',
              },
            },
            required: ['name', 'photoUrl'],
            title: 'PetRequest',
            type: 'object',
          },
        },
      }

      const result = convertSchemaToJson(rootSchema, rootSchema)
      expect(result).toEqual({
        pet: {
          name: 'string',
          photoUrl: 'string',
        },
        foodOrder: {
          quantity: 0,
        },
      })
    })
  })

  describe('Edge cases and type combinations', () => {
    it('should handle object with all primitive types', () => {
      const schema = {
        type: 'object',
        properties: {
          stringField: { type: 'string' },
          numberField: { type: 'number' },
          integerField: { type: 'integer' },
          booleanField: { type: 'boolean' },
          nullField: { type: 'null' },
        },
      }
      const result = convertSchemaToJson(schema)
      expect(result).toEqual({
        stringField: 'string',
        numberField: 0,
        integerField: 0,
        booleanField: false,
        nullField: null,
      })
    })

    it('should handle empty array', () => {
      const schema = {
        type: 'array',
        items: { type: 'string' },
      }
      const result = convertSchemaToJson(schema)
      expect(result).toEqual(['string'])
    })

    it('should handle array without items', () => {
      const schema = {
        type: 'array',
      }
      const result = convertSchemaToJson(schema)
      expect(result).toEqual([])
    })

    it('should handle object without properties', () => {
      const schema = {
        type: 'object',
      }
      const result = convertSchemaToJson(schema)
      expect(result).toEqual({})
    })

    it('should handle $ref without rootSchema', () => {
      const schema = {
        $ref: '#/$defs/SomeDef',
      }
      const result = convertSchemaToJson(schema)
      expect(result).toEqual({})
    })

    it('should handle invalid $ref path', () => {
      const rootSchema = {
        $defs: {
          ValidDef: { type: 'string' },
        },
      }
      const schema = {
        $ref: '#/$defs/InvalidDef',
      }
      const result = convertSchemaToJson(schema, rootSchema)
      expect(result).toEqual({})
    })

    it('should handle $ref that does not start with #/$defs/', () => {
      const rootSchema = {
        $defs: {
          SomeDef: { type: 'string' },
        },
      }
      const schema = {
        $ref: '#/components/schemas/SomeDef',
      }
      const result = convertSchemaToJson(schema, rootSchema)
      expect(result).toEqual({})
    })

    it('should handle anyOf with multiple non-null schemas', () => {
      const schema = {
        anyOf: [{ type: 'string' }, { type: 'number' }],
      }
      const result = convertSchemaToJson(schema)
      expect(result).toBe('string')
    })

    it('should handle deeply nested $ref references', () => {
      const rootSchema = {
        type: 'object',
        properties: {
          level1: {
            $ref: '#/$defs/Level1',
          },
        },
        $defs: {
          Level1: {
            type: 'object',
            properties: {
              level2: {
                $ref: '#/$defs/Level2',
              },
            },
          },
          Level2: {
            type: 'object',
            properties: {
              level3: {
                $ref: '#/$defs/Level3',
              },
            },
          },
          Level3: {
            type: 'object',
            properties: {
              value: { type: 'string' },
            },
          },
        },
      }

      const result = convertSchemaToJson(rootSchema, rootSchema)
      expect(result).toEqual({
        level1: {
          level2: {
            level3: {
              value: 'string',
            },
          },
        },
      })
    })
  })
})
