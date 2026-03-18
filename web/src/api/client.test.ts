import { describe, it, expect, beforeAll, afterAll, afterEach } from 'vitest'
import { setupServer } from 'msw/node'
import { http, HttpResponse } from 'msw'
import { getHealth } from './client'
import type { ApiHealthResponse } from './models'

const handlers = [
  http.get('*/api/v1/health', () => {
    const response: ApiHealthResponse = {
      status: 'ok',
      version: '1.0.0'
    }
    return HttpResponse.json(response)
  })
]

const server = setupServer(...handlers)

describe('ApiClient', () => {
  beforeAll(() => server.listen())
  afterEach(() => server.resetHandlers())
  afterAll(() => server.close())

  it('getHealth returns health status', async () => {
    const health = await getHealth()
    expect(health.status).toBe('ok')
    expect(health.version).toBe('1.0.0')
  })

  it('getHealth handles errors correctly', async () => {
    server.use(
      http.get('*/api/v1/health', () => {
        return new HttpResponse(null, { status: 500 })
      })
    )

    await expect(getHealth()).rejects.toThrow('API request failed with HTTP 500.')
  })
})
