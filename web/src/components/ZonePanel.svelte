<script lang="ts">
  import { api } from '../lib/api'
  import { sourceLabel } from '../lib/live.svelte'
  import { probeForSource, thermalColor } from '../lib/thermal'
  import type { FanReading, ZoneStatus } from '../lib/types'
  import Rotor from './Rotor.svelte'

  let { zone, fans }: { zone: ZoneStatus; fans: FanReading[] } = $props()

  // Supermicro naming: FAN1..n belong to zone 0 (CPU), FANA..n to zone 1 (peripheral).
  const zoneFans = $derived(
    fans.filter((f) =>
      zone.id === 0 ? /^FAN\d/i.test(f.name) : zone.id === 1 ? /^FAN[A-Z]$/i.test(f.name) : false,
    ),
  )
  const rpm = $derived(Math.max(0, ...zoneFans.map((f) => f.rpm ?? 0)) || null)
  const driver = $derived(zone.rules.find((r) => r.source === zone.driver))

  let manualDuty = $state(60)
  let editing = $state(false)
  let error = $state<string | null>(null)

  const modeText = {
    auto: 'Automatic',
    manual: 'Manual',
    failsafe: 'Failsafe — a sensor is missing',
    unmanaged: 'Not controlled',
  } as const

  async function setManual(duty: number | null) {
    error = null
    try {
      await api.setManual(zone.id, duty)
      editing = false
    } catch (e) {
      error = e instanceof Error ? e.message : String(e)
    }
  }
</script>

<section class="zone panel" data-mode={zone.mode} aria-labelledby="zone-{zone.id}">
  <header>
    <h2 id="zone-{zone.id}">{zone.name}</h2>
    <span class="mode">{modeText[zone.mode]}</span>
  </header>

  <div class="readout">
    <Rotor {rpm} size={76} />
    <div>
      <div class="duty figure">{zone.duty ?? '—'}<small>%</small></div>
      <div class="soft">{rpm ? `${rpm} RPM` : 'no fan reading'}</div>
    </div>
  </div>

  {#if zone.rules.length}
    <ul class="rules">
      {#each zone.rules as rule (rule.source)}
        <li class:driving={rule === driver}>
          <span
            class="swatch"
            style:background={thermalColor(rule.temperature, probeForSource(rule.source))}
            aria-hidden="true"
          ></span>
          <span class="label">{sourceLabel(rule.source)}</span>
          <span class="figure">
            {#if rule.state === 'ok'}{rule.temperature}°C
            {:else if rule.state === 'idle'}<span class="soft">asleep</span>
            {:else}<span class="missing">missing</span>{/if}
          </span>
          <span class="figure soft">{rule.duty}%</span>
        </li>
      {/each}
    </ul>
  {/if}

  <footer>
    {#if zone.mode === 'manual'}
      <button class="quiet" onclick={() => setManual(null)}>Return to automatic</button>
    {/if}
    {#if editing}
      <form
        class="manual"
        onsubmit={(e) => {
          e.preventDefault()
          setManual(manualDuty)
        }}
      >
        <label class="visually-hidden" for="duty-{zone.id}">Fan speed for {zone.name}</label>
        <input id="duty-{zone.id}" type="range" min="0" max="100" step="1" bind:value={manualDuty} />
        <span class="figure">{manualDuty}%</span>
        <button type="submit">Apply</button>
        <button type="button" class="quiet" onclick={() => (editing = false)}>Cancel</button>
      </form>
    {:else}
      <button
        class="quiet"
        onclick={() => {
          manualDuty = zone.duty ?? 60
          editing = true
        }}>Set speed manually</button
      >
    {/if}
    {#if error}<p class="missing">{error}</p>{/if}
  </footer>
</section>

<style>
  .zone {
    display: grid;
    gap: 1rem;
    align-content: start;
  }

  header {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 1rem;
    flex-wrap: wrap;
  }

  .mode {
    font-size: var(--step--1);
    color: var(--ink-soft);
  }

  [data-mode='manual'] .mode {
    color: var(--ink);
    font-weight: 600;
  }

  [data-mode='failsafe'] {
    border-color: var(--critical);
  }

  [data-mode='failsafe'] .mode {
    color: var(--critical);
    font-weight: 600;
  }

  .readout {
    display: flex;
    align-items: center;
    gap: 1.1rem;
  }

  .duty {
    font-size: var(--step-4);
    line-height: 0.95;
    letter-spacing: -0.01em;
  }

  .duty small {
    font-size: 0.45em;
    margin-left: 0.08em;
    color: var(--ink-soft);
  }

  .rules {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 0.35rem;
  }

  .rules li {
    display: grid;
    grid-template-columns: 0.6rem 1fr auto 3.2rem;
    align-items: center;
    gap: 0.6rem;
    color: var(--ink-soft);
  }

  .rules li.driving {
    color: var(--ink);
    font-weight: 500;
  }

  .rules .figure {
    text-align: right;
  }

  .swatch {
    width: 0.6rem;
    height: 0.6rem;
    border-radius: 2px;
  }

  .label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .missing {
    color: var(--critical);
  }

  footer {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
    align-items: center;
  }

  .manual {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.6rem;
    width: 100%;
  }

  .manual input {
    flex: 1 1 140px;
  }
</style>
