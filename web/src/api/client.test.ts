import { describe, it, expect, beforeAll, afterAll, afterEach, vi } from 'vitest'
import { setupServer } from 'msw/node'
import { http, HttpResponse } from 'msw'
import { buildUrl, getHealth } from './client'
import type { ApiHealthResponse } from './models'

const handlers = [
  http.get('*/api/v1/health', () => {
    const response: ApiHealthResponse = {
      status: 'ok',
    }
    return HttpResponse.json(response)
  })
]

const server = setupServer(...handlers)

describe('ApiClient', () => {
  beforeAll(() => server.listen())
  afterEach(() => {
    server.resetHandlers()
    vi.unstubAllEnvs()
  })
  afterAll(() => server.close())

  it('buildUrl uses the Vite API base URL when configured', () => {
    vi.stubEnv('VITE_API_BASE_URL', 'http://localhost:3450/')

    expect(buildUrl('/status')).toBe('http://localhost:3450/api/v1/status')
  })

  it('buildUrl falls back to the dev proxy path when the API base URL is empty', () => {
    vi.stubEnv('VITE_API_BASE_URL', '   ')

    expect(buildUrl('/status')).toBe('/api/v1/status')
  })

  it('getHealth returns health status', async () => {
    const health = await getHealth()
    expect(health.status).toBe('ok')
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
