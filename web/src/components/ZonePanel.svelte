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

  const modeBadge = {
    auto: 'badge-success',
    manual: 'badge-info',
    failsafe: 'badge-error',
    unmanaged: 'badge-ghost',
    calibrating: 'badge-warning',
  } as const

  const modeText = {
    auto: 'Automatic',
    manual: 'Manual',
    failsafe: 'Failsafe — a sensor is missing',
    unmanaged: 'Not controlled',
    calibrating: 'Calibrating',
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

<section class="card border border-base-300 bg-base-100" aria-labelledby="zone-{zone.id}">
  <div class="card-body gap-4">
    <header class="flex flex-wrap items-center justify-between gap-2">
      <h2 id="zone-{zone.id}" class="card-title">{zone.name}</h2>
      <span class="badge badge-soft {modeBadge[zone.mode]}">{modeText[zone.mode]}</span>
    </header>

    <div class="flex items-center gap-5">
      <Rotor {rpm} size={76} />
      <div>
        <div class="font-figures text-5xl leading-none font-semibold tabular-nums">
          {zone.duty ?? '—'}<span class="text-2xl text-base-content/50">%</span>
        </div>
        <div class="text-base-content/60">{rpm ? `${rpm} RPM` : 'no fan reading'}</div>
      </div>
    </div>

    {#if zone.rules.length}
      <table class="table table-sm">
        <thead>
          <tr><th>Source</th><th class="text-right">Temp.</th><th class="text-right">Asks</th></tr>
        </thead>
        <tbody>
          {#each zone.rules as rule, i (i)}
            <tr class={rule === driver ? 'bg-base-200 font-semibold' : ''}>
              <td>
                <span
                  class="status mr-1.5"
                  style:background-color={thermalColor(rule.temperature, probeForSource(rule.source))}
                ></span>
                {sourceLabel(rule.source)}
                {#if rule.target !== null}
                  <span class="badge badge-xs ml-1 {rule.emergency ? 'badge-error' : 'badge-ghost'}"
                    >{rule.emergency ? 'far above target' : 'target'} {rule.target}°</span
                  >
                {/if}
              </td>
              <td class="text-right font-figures tabular-nums">
                {#if rule.state === 'ok'}{rule.temperature}°C
                {:else if rule.state === 'idle'}<span class="text-base-content/60">asleep</span>
                {:else}<span class="text-error">missing</span>{/if}
              </td>
              <td class="text-right font-figures text-base-content/70 tabular-nums">{rule.duty}%</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}

    <footer class="card-actions items-center">
      {#if zone.mode === 'calibrating'}
        <p class="text-base-content/60">Calibration in progress: see the Curves page.</p>
      {:else if editing}
        <form
          class="flex w-full flex-wrap items-center gap-3"
          onsubmit={(e) => {
            e.preventDefault()
            setManual(manualDuty)
          }}
        >
          <label class="sr-only" for="duty-{zone.id}">Fan speed for {zone.name}</label>
          <input
            id="duty-{zone.id}"
            class="range range-sm min-w-40 flex-1"
            type="range"
            min="0"
            max="100"
            step="1"
            bind:value={manualDuty}
          />
          <span class="w-12 text-right font-figures font-semibold tabular-nums">{manualDuty}%</span>
          <button class="btn btn-primary btn-sm" type="submit">Apply</button>
          <button type="button" class="btn btn-ghost btn-sm" onclick={() => (editing = false)}>Cancel</button>
        </form>
      {:else}
        {#if zone.mode === 'manual'}
          <button class="btn btn-sm" onclick={() => setManual(null)}>Return to automatic</button>
        {/if}
        <button
          class="btn btn-sm"
          onclick={() => {
            manualDuty = zone.duty ?? 60
            editing = true
          }}>Set speed manually</button
        >
      {/if}
      {#if error}<p class="w-full text-error">{error}</p>{/if}
    </footer>
  </div>
</section>
