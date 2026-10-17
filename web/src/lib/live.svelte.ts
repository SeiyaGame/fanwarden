import type { Snapshot } from './types'

/** Latest snapshot pushed by the server over a WebSocket, with automatic reconnection. */
class Live {
  snapshot = $state<Snapshot | null>(null)
  connected = $state(false)
  private socket: WebSocket | null = null
  private retry: ReturnType<typeof setTimeout> | null = null
  private stopped = true

  start() {
    this.stopped = false
    this.connect()
  }

  stop() {
    this.stopped = true
    if (this.retry) clearTimeout(this.retry)
    this.socket?.close()
    this.socket = null
    this.connected = false
  }

  private connect() {
    const protocol = location.protocol === 'https:' ? 'wss:' : 'ws:'
    const socket = new WebSocket(`${protocol}//${location.host}/api/live`)
    this.socket = socket
    socket.onopen = () => (this.connected = true)
    socket.onmessage = (event) => {
      this.snapshot = JSON.parse(event.data) as Snapshot
    }
    socket.onclose = () => {
      this.connected = false
      if (!this.stopped) this.retry = setTimeout(() => this.connect(), 3000)
    }
  }
}

export const live = new Live()

/** Display name of a source id, from the list the server sends. */
export function sourceLabel(id: string): string {
  return live.snapshot?.sources.find((s) => s.id === id)?.label ?? id
}
