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

<section class="panel" aria-labelledby="bmc-events-title">
  <header>
    <h2 id="bmc-events-title">BMC event log</h2>
    <p class="soft">
      What the motherboard recorded: memory errors, power supplies, chassis intrusion, temperatures and fans
      crossing their thresholds. Dates come from the BMC clock.
      {#if log?.percent_used != null}The log is {log.percent_used}% full.{/if}
    </p>
    {#if log?.percent_used != null && log.percent_used >= 90}
      <p class="fault">
        Almost full: the BMC may stop recording. Once you have read it, clear it with
        <code>ipmitool sel clear</code>.
      </p>
    {/if}
  </header>

  {#if error ?? log?.error}
    <p class="fault">{error ?? log?.error}</p>
  {:else if log === null || log.updated === null}
    <p class="soft">Reading the log…</p>
  {/if}

  {#if log?.updated}
    <ol class="entries">
      {#each shown as entry (entry.id)}
        <li data-severity={entry.severity}>
          <time class="soft figure">{entry.date} {entry.time}</time>
          <span>
            <strong>{entry.sensor}</strong>: {entry.event}
            <span class="soft">{note(entry)}</span>
          </span>
          <span class="badge">{SEVERITY_TEXT[entry.severity]}</span>
        </li>
      {:else}
        <li class="soft">{filter === 'all' ? 'The log is empty.' : 'No warning or failure.'}</li>
      {/each}
    </ol>
  {/if}
</section>

<style>
  header {
    margin-bottom: 1rem;
  }

  header p {
    margin: 0.35rem 0 0;
  }

  .entries {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 0.45rem;
    overflow-wrap: anywhere;
  }

  .entries li {
    display: grid;
    grid-template-columns: 11.5rem 1fr auto;
    gap: 1rem;
    align-items: baseline;
    padding-left: 0.6rem;
    border-left: 3px solid transparent;
  }

  .entries li[data-severity='warning'] {
    border-left-color: var(--warm);
  }

  .entries li[data-severity='critical'] {
    border-left-color: var(--critical);
  }

  time {
    font-weight: 500;
  }

  .badge {
    font-size: var(--step--1);
    color: var(--ink-soft);
  }

  li[data-severity='critical'] .badge {
    color: var(--critical);
    font-weight: 600;
  }

  @media (max-width: 560px) {
    .entries li {
      grid-template-columns: 1fr auto;
      gap: 0 1rem;
    }

    time {
      grid-column: 1 / -1;
    }
  }
</style>
