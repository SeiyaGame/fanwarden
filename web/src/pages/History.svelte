<script lang="ts">
  import Chart from '../components/Chart.svelte'
  import { api } from '../lib/api'
  import { live, sourceLabel } from '../lib/live.svelte'
  import type { HistoryData } from '../lib/types'

  const RANGES = [
    { label: '1 hour', seconds: 3600 },
    { label: '6 hours', seconds: 6 * 3600 },
    { label: '24 hours', seconds: 86400 },
    { label: '7 days', seconds: 7 * 86400 },
    { label: '30 days', seconds: 30 * 86400 },
  ]

  let range = $state(3600)
  let data = $state<{
    temps: HistoryData
    disks: HistoryData
    duty: HistoryData
    fans: HistoryData
    power: HistoryData
  } | null>(null)
  let error = $state<string | null>(null)

  async function load(seconds: number) {
    error = null
    try {
      const [temps, disks, duty, fans, power] = await Promise.all([
        api.history(seconds, 'temp:cpu,temp:ipmi:'),
        api.history(seconds, 'temp:disk:'),
        api.history(seconds, 'duty:'),
        api.history(seconds, 'fan:'),
        api.history(seconds, 'power'),
      ])
      data = { temps, disks, duty, fans, power }
    } catch (e) {
      error = e instanceof Error ? e.message : String(e)
    }
  }

  $effect(() => {
    load(range)
    const timer = setInterval(() => load(range), 60_000)
    return () => clearInterval(timer)
  })

  function label(key: string): string {
    if (key.startsWith('temp:')) return sourceLabel(key.slice(5)).replace(/^Disk /, '')
    if (key.startsWith('duty:')) {
      const id = Number(key.slice(5))
      return live.snapshot?.zones.find((z) => z.id === id)?.name ?? `Zone ${id}`
    }
    if (key === 'power') return 'Power draw'
    return key.replace(/^fan:/, '')
  }
</script>

<div class="page">
  <header class="head">
    <h1>History</h1>
    <div class="ranges" role="group" aria-label="Time range">
      {#each RANGES as r (r.seconds)}
        <button
          class:quiet={range !== r.seconds}
          aria-pressed={range === r.seconds}
          onclick={() => (range = r.seconds)}>{r.label}</button
        >
      {/each}
    </div>
  </header>

  {#if error}
    <p class="error">Cannot load the history: {error}</p>
  {:else if !data}
    <p class="soft">Loading…</p>
  {:else if data.temps.timestamps.length === 0}
    <p class="soft">No data for this period yet. Readings are recorded at every control cycle.</p>
  {:else}
    <section class="panel">
      <h2>CPU and board</h2>
      <Chart data={data.temps} {label} unit="°C" />
    </section>
    {#if data.disks.series.length}
      <section class="panel">
        <h2>Disks</h2>
        <Chart data={data.disks} {label} unit="°C" />
      </section>
    {/if}
    <section class="panel">
      <h2>Zone speed</h2>
      <Chart data={data.duty} {label} unit="%" />
    </section>
    <section class="panel">
      <h2>Fans</h2>
      <Chart data={data.fans} {label} unit=" rpm" />
    </section>
    {#if data.power.series.length}
      <section class="panel">
        <h2>Power</h2>
        <Chart data={data.power} {label} unit=" W" />
      </section>
    {/if}
  {/if}
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

  .ranges {
    display: flex;
    flex-wrap: wrap;
    gap: 0.35rem;
  }

  section.panel {
    display: grid;
    gap: 0.75rem;
  }

  .error {
    color: var(--critical);
  }
</style>
