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

<div class="grid gap-6">
  <header class="flex flex-wrap items-center justify-between gap-4">
    <h1 class="text-2xl font-semibold">History</h1>
    <div class="tabs tabs-box tabs-sm" role="group" aria-label="Time range">
      {#each RANGES as r (r.seconds)}
        <button
          class="tab {range === r.seconds ? 'tab-active' : ''}"
          aria-pressed={range === r.seconds}
          onclick={() => (range = r.seconds)}>{r.label}</button
        >
      {/each}
    </div>
  </header>

  {#if error}
    <div role="alert" class="alert alert-error alert-soft">Cannot load the history: {error}</div>
  {:else if !data}
    <p class="flex items-center gap-2 text-base-content/60">
      <span class="loading loading-spinner loading-sm"></span> Loading…
    </p>
  {:else if data.temps.timestamps.length === 0}
    <div role="status" class="alert alert-soft">
      No data for this period yet. Readings are recorded at every control cycle.
    </div>
  {:else}
    {#snippet chart(title: string, series: HistoryData, unit: string)}
      <section class="card border border-base-300 bg-base-100">
        <div class="card-body">
          <h2 class="card-title">{title}</h2>
          <Chart data={series} {label} {unit} />
        </div>
      </section>
    {/snippet}
    {@render chart('CPU and board', data.temps, '°C')}
    {#if data.disks.series.length}{@render chart('Disks', data.disks, '°C')}{/if}
    {@render chart('Zone speed', data.duty, '%')}
    {@render chart('Fans', data.fans, ' rpm')}
    {#if data.power.series.length}{@render chart('Power', data.power, ' W')}{/if}
  {/if}
</div>
