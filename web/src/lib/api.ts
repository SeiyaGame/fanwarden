import type { Config, HistoryData, LogEvent, NotificationChannel, Snapshot } from './types'

export class ApiError extends Error {
  constructor(
    public status: number,
    message: string,
  ) {
    super(message)
  }
}

/** Called when a request comes back 401: the session expired. */
let onUnauthorized: () => void = () => {}
export function setUnauthorizedHandler(handler: () => void) {
  onUnauthorized = handler
}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
  const response = await fetch(`/api${path}`, {
    method,
    headers: body === undefined ? {} : { 'Content-Type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
    credentials: 'same-origin',
  })
  if (response.status === 401 && !path.startsWith('/auth/')) onUnauthorized()
  const text = await response.text()
  let data: unknown
  try {
    data = text ? JSON.parse(text) : null
  } catch {
    data = text
  }
  if (!response.ok) {
    const message =
      data && typeof data === 'object' && 'error' in data
        ? String(data.error)
        : String(data || response.statusText)
    throw new ApiError(response.status, message)
  }
  return data as T
}

export const api = {
  authStatus: () => request<{ setup_required: boolean; authenticated: boolean }>('GET', '/auth/status'),
  setup: (password: string) => request('POST', '/auth/setup', { password }),
  login: (password: string) => request('POST', '/auth/login', { password }),
  logout: () => request('POST', '/auth/logout'),
  changePassword: (current: string, next: string) =>
    request('POST', '/auth/password', { current, new: next }),
  status: () => request<Snapshot>('GET', '/status'),
  config: () => request<Config>('GET', '/config'),
  saveConfig: (config: Config) => request<Config>('PUT', '/config', config),
  reloadConfig: () => request<Config>('POST', '/config/reload'),
  setManual: (zone: number, duty: number | null) => request('PUT', `/zones/${zone}/manual`, { duty }),
  startCalibration: (zone: number) => request('POST', `/zones/${zone}/calibration`),
  cancelCalibration: (zone: number) => request('DELETE', `/zones/${zone}/calibration`),
  events: (after = 0) => request<LogEvent[]>('GET', `/events?after=${after}`),
  history: (range: number, prefix: string, points = 600) =>
    request<HistoryData>(
      'GET',
      `/history?range=${range}&points=${points}&prefix=${encodeURIComponent(prefix)}`,
    ),
  testNotification: (channel: NotificationChannel, index: number | null) =>
    request('POST', '/notifications/test', { channel, index }),
}
