<script lang="ts">
  // Hardware event log of the BMC (SEL), read by the service every minute.
  import { api } from '../lib/api'
  import type { BmcEvent, BmcLog } from '../lib/types'

  let { filter }: { filter: 'all' | 'problems' } = $props()

  let log = $state<BmcLog | null>(null)
  let error = $state<string | null>(null)
  const shown = $derived(
    (log?.entries ?? []).filter((e) => filter === 'all' || e.severity !== 'info').toReversed(),
  )

  async function load() {
    try {
      log = await api.bmcEvents()
      error = null
    } catch (e) {
      error = e instanceof Error ? e.message : String(e)
    }
  }

  $effect(() => {
    load()
    const timer = setInterval(load, 30_000)
    return () => clearInterval(timer)
  })

  /** Details and "cleared", after the event text. */
  const note = (entry: BmcEvent) =>
    [entry.details, entry.direction === 'Deasserted' && 'cleared']
      .filter(Boolean)
      .map((text) => `(${text})`)
      .join(' ')

  const SEVERITY_TEXT = { info: 'Info', warning: 'Warning', critical: 'Failure' }
</script>

<section class="card border border-base-300 bg-base-100" aria-labelledby="bmc-events-title">
  <div class="card-body gap-4">
    <header class="grid gap-2">
      <h2 id="bmc-events-title" class="card-title">BMC event log</h2>
      <p class="text-sm text-base-content/70">
        What the motherboard recorded: memory errors, power supplies, chassis intrusion, temperatures and fans
        crossing their thresholds. Dates come from the BMC clock.
      </p>
      {#if log?.percent_used != null}
        <div class="flex items-center gap-3 text-sm">
          <progress
            class="progress w-40 {log.percent_used >= 90 ? 'progress-error' : 'progress-info'}"
            value={log.percent_used}
            max="100"
          ></progress>
          <span class="text-base-content/70">{log.percent_used}% full</span>
        </div>
      {/if}
      {#if log?.percent_used != null && log.percent_used >= 90}
        <div role="alert" class="alert alert-error alert-soft">
          <span
            >Almost full: the BMC may stop recording. Once you have read it, clear it with
            <code class="font-mono">ipmitool sel clear</code>.</span
          >
        </div>
      {/if}
    </header>

    {#if error ?? log?.error}
      <div role="alert" class="alert alert-error alert-soft">{error ?? log?.error}</div>
    {:else if log === null || log.updated === null}
      <p class="flex items-center gap-2 text-base-content/60">
        <span class="loading loading-spinner loading-sm"></span> Reading the log…
      </p>
    {/if}

    {#if log?.updated}
      <ul class="list rounded-box border border-base-300 [overflow-wrap:anywhere]">
        {#each shown as entry (entry.id)}
          <li class="list-row items-center py-2.5">
            <span
              class="status {entry.severity === 'critical'
                ? 'status-error'
                : entry.severity === 'warning'
                  ? 'status-warning'
                  : 'status-neutral'}"
            ></span>
            <div class="list-col-grow">
              <div>
                <strong>{entry.sensor}</strong>: {entry.event}
                <span class="text-base-content/60">{note(entry)}</span>
              </div>
              <time class="font-figures text-xs text-base-content/60 tabular-nums"
                >{entry.date} {entry.time}</time
              >
            </div>
            <span
              class="badge badge-sm {entry.severity === 'critical'
                ? 'badge-error'
                : entry.severity === 'warning'
                  ? 'badge-warning'
                  : 'badge-ghost'}">{SEVERITY_TEXT[entry.severity]}</span
            >
          </li>
        {:else}
          <li class="list-row text-base-content/60">
            {filter === 'all' ? 'The log is empty.' : 'No warning or failure.'}
          </li>
        {/each}
      </ul>
    {/if}
  </div>
</section>
