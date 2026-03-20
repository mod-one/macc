import { describe, it, expect, beforeAll, afterAll, afterEach, vi } from 'vitest'
import { setupServer } from 'msw/node'
import { http, HttpResponse } from 'msw'
import { buildUrl, getHealth, getStatus } from './client'
import type { ApiCoordinatorStatus, ApiHealthResponse } from './models'

const handlers = [
  http.get('*/api/v1/health', () => {
    const response: ApiHealthResponse = {
      status: 'ok',
    }
    return HttpResponse.json(response)
  }),
  http.get('*/api/v1/status', () => {
    const response: ApiCoordinatorStatus = {
      total: 12,
      todo: 4,
      active: 2,
      blocked: 1,
      merged: 5,
      paused: false,
      pause_reason: null,
      pause_task_id: null,
      pause_phase: null,
      latest_error: null,
      failure_report: null,
      throttled_tools: [],
      effective_max_parallel: 3,
    }
    return HttpResponse.json(response)
  }),
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

  it('getStatus requests and parses coordinator status', async () => {
    const status = await getStatus()

    expect(status).toMatchObject({
      total: 12,
      active: 2,
      blocked: 1,
      merged: 5,
      paused: false,
    })
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
