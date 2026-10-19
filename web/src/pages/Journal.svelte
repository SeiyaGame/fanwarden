<script lang="ts">
  import BmcEvents from '../components/BmcEvents.svelte'
  import ConfigHistory from '../components/ConfigHistory.svelte'
  import { api } from '../lib/api'
  import type { LogEvent } from '../lib/types'

  let events = $state<LogEvent[]>([])
  let view = $state<'events' | 'bmc' | 'config'>('events')
  let filter = $state<'all' | 'problems'>('all')
  const shown = $derived((filter === 'all' ? events : events.filter((e) => e.level !== 'info')).toReversed())

  async function poll() {
    const after = events.at(-1)?.id ?? 0
    try {
      const fresh = await api.events(after)
      if (fresh.length) events = [...events, ...fresh].slice(-1000)
    } catch {
      // Offline: the next poll retries.
    }
  }

  $effect(() => {
    poll()
    const timer = setInterval(poll, 5000)
    return () => clearInterval(timer)
  })

  const time = (ts: number) => new Date(ts * 1000).toLocaleString()
</script>

<div class="grid gap-6">
  <header class="flex flex-wrap items-center justify-between gap-4">
    <h1 class="text-2xl font-semibold">Journal</h1>
    <div role="group" aria-label="What to show" class="tabs tabs-box tabs-sm">
      {#each [['events', 'Events'], ['bmc', 'BMC hardware log'], ['config', 'Configuration changes']] as const as [key, text] (key)}
        <button
          class="tab {view === key ? 'tab-active' : ''}"
          aria-pressed={view === key}
          onclick={() => (view = key)}>{text}</button
        >
      {/each}
    </div>
  </header>

  {#if view === 'config'}
    <ConfigHistory />
  {:else}
    <div role="group" aria-label="Filter" class="join">
      <button
        class="btn join-item btn-sm {filter === 'all' ? 'btn-primary' : ''}"
        aria-pressed={filter === 'all'}
        onclick={() => (filter = 'all')}>Everything</button
      >
      <button
        class="btn join-item btn-sm {filter === 'problems' ? 'btn-primary' : ''}"
        aria-pressed={filter === 'problems'}
        onclick={() => (filter = 'problems')}>Warnings and errors</button
      >
    </div>

    {#if view === 'bmc'}
      <BmcEvents {filter} />
    {:else}
      <ol class="list rounded-box border border-base-300 bg-base-100 [overflow-wrap:anywhere]">
        {#each shown as event (event.id)}
          <li class="list-row items-center py-2.5">
            <span
              class="status {event.level === 'error'
                ? 'status-error'
                : event.level === 'warn'
                  ? 'status-warning'
                  : 'status-neutral'}"
            ></span>
            <div class="list-col-grow grid gap-x-4 sm:grid-cols-[11.5rem_1fr]">
              <time class="font-figures text-sm text-base-content/60 tabular-nums">{time(event.ts)}</time>
              <span>{event.message}</span>
            </div>
          </li>
        {:else}
          <li class="list-row text-base-content/60">Nothing recorded since the service started.</li>
        {/each}
      </ol>
    {/if}
  {/if}
</div>
