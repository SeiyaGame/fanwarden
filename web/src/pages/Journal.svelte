<script lang="ts">
  import { api } from '../lib/api'
  import type { LogEvent } from '../lib/types'

  let events = $state<LogEvent[]>([])
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

<div class="page">
  <header class="head">
    <h1>Journal</h1>
    <div role="group" aria-label="Filter" class="filters">
      <button class:quiet={filter !== 'all'} aria-pressed={filter === 'all'} onclick={() => (filter = 'all')}
        >Everything</button
      >
      <button
        class:quiet={filter !== 'problems'}
        aria-pressed={filter === 'problems'}
        onclick={() => (filter = 'problems')}>Warnings and errors</button
      >
    </div>
  </header>

  <ol class="panel log">
    {#each shown as event (event.id)}
      <li data-level={event.level}>
        <time class="soft figure">{time(event.ts)}</time>
        <span>{event.message}</span>
      </li>
    {:else}
      <li class="soft">Nothing recorded since the service started.</li>
    {/each}
  </ol>
</div>

<style>
  .page {
    display: grid;
    gap: var(--gutter);
  }

  .head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    flex-wrap: wrap;
    gap: 1rem;
  }

  .filters {
    display: flex;
    gap: 0.35rem;
  }

  .log {
    list-style: none;
    margin: 0;
    display: grid;
    gap: 0.45rem;
    overflow-wrap: anywhere;
  }

  .log li {
    display: grid;
    grid-template-columns: 11.5rem 1fr;
    gap: 1rem;
    padding-left: 0.6rem;
    border-left: 3px solid transparent;
  }

  .log li[data-level='warn'] {
    border-left-color: var(--warm);
  }

  .log li[data-level='error'] {
    border-left-color: var(--critical);
  }

  .log time {
    font-weight: 500;
  }

  @media (max-width: 560px) {
    .log li {
      grid-template-columns: 1fr;
      gap: 0;
    }
  }
</style>
