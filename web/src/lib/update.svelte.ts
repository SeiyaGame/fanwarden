import { api } from './api'
import type { UpdateStatus } from './types'

/** Latest release seen by the service, refreshed every hour while the UI is open. */
class UpdateStore {
  status = $state<UpdateStatus | null>(null)
  private timer: ReturnType<typeof setInterval> | undefined

  async load() {
    try {
      this.status = await api.update()
    } catch {
      // Offline: the next refresh retries.
    }
  }

  start() {
    this.load()
    this.timer = setInterval(() => this.load(), 3600_000)
  }

  stop() {
    clearInterval(this.timer)
  }
}

export const updates = new UpdateStore()
