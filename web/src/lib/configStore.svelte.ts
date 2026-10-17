import { api } from './api'
import type { Config } from './types'

/** The configuration being edited. Every settings page edits the same draft; one bar saves it. */
class ConfigStore {
  saved = $state<Config | null>(null)
  draft = $state<Config | null>(null)
  saving = $state(false)
  error = $state<string | null>(null)
  savedAt = $state<number | null>(null)

  dirty = $derived(
    this.saved !== null && this.draft !== null && JSON.stringify(this.saved) !== JSON.stringify(this.draft),
  )

  async load() {
    const config = await api.config()
    this.saved = config
    this.draft = structuredClone(config)
    this.error = null
  }

  async save() {
    if (!this.draft) return
    this.saving = true
    this.error = null
    try {
      const config = await api.saveConfig($state.snapshot(this.draft) as Config)
      this.saved = config
      this.draft = structuredClone(config)
      this.savedAt = Date.now()
    } catch (e) {
      this.error = e instanceof Error ? e.message : String(e)
    } finally {
      this.saving = false
    }
  }

  discard() {
    if (this.saved) this.draft = structuredClone($state.snapshot(this.saved) as Config)
    this.error = null
  }
}

export const configStore = new ConfigStore()
