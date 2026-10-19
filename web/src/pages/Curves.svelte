<script lang="ts">
  import CalibrationPanel from '../components/CalibrationPanel.svelte'
  import CurveEditor from '../components/CurveEditor.svelte'
  import { configStore } from '../lib/configStore.svelte'
  import { live } from '../lib/live.svelte'
  import { probeForSource, type Probe } from '../lib/thermal'
  import type { Zone } from '../lib/types'

  const config = $derived(configStore.draft)
  const sources = $derived(live.snapshot?.sources ?? [])

  function diskKind(serial: string): Probe | undefined {
    return live.snapshot?.sensors.disks.find((d) => d.serial === serial)?.kind
  }

  function currentTemp(source: string): number | null {
    return sources.find((s) => s.id === source)?.celsius ?? null
  }

  /** Same split as the backend (src/regulator.rs): CPU sensors are fast, the rest slow. */
  function safetyMargin(source: string): number {
    const regulation = configStore.draft?.regulation
    if (!regulation) return 4
    return probeForSource(source, diskKind) === 'cpu'
      ? regulation.safety_margin_cpu
      : regulation.safety_margin_disks
  }

  function defaultTarget(source: string): number {
    const targets: Record<Probe, number> = { cpu: 65, hdd: 38, ssd: 45, nvme: 55, board: 45 }
    return targets[probeForSource(source, diskKind)]
  }

  /** Duty the regulator chose for this rule in the last cycle. */
  function liveDuty(zoneId: number, index: number): number | null {
    return live.snapshot?.zones.find((z) => z.id === zoneId)?.rules[index]?.duty ?? null
  }

  function addRule(zone: Zone) {
    const used = new Set(zone.rules.map((r) => r.source))
    const source = sources.find((s) => !used.has(s.id))?.id ?? 'cpu'
    zone.rules.push({
      source,
      curve: [
        [30, 30],
        [60, 100],
      ],
    })
  }

  function addZone() {
    if (!config) return
    const id = Math.max(-1, ...config.zones.map((z) => z.id)) + 1
    config.zones.push({ id, name: `Zone ${id}`, hysteresis: 2, min_duty: 20, max_duty: 100, rules: [] })
  }
</script>

<div class="grid gap-6">
  <header class="grid max-w-[70ch] gap-2">
    <h1 class="text-2xl font-semibold">Fan curves</h1>
    <p class="text-base-content/70">
      Each zone follows the hottest of its sources: every source has its own curve, and the zone runs at the
      highest speed they ask for. A zone whose source cannot be read runs at the failsafe speed.
    </p>
  </header>

  {#if config}
    {#each config.zones as zone, zi (zi)}
      <section class="card border border-base-300 bg-base-100" aria-labelledby="zone-title-{zi}">
        <div class="card-body gap-5">
          <header class="flex flex-wrap items-center justify-between gap-4">
            <h2 id="zone-title-{zi}" class="card-title">{zone.name || `Zone ${zone.id}`}</h2>
            <button
              class="btn btn-sm btn-error btn-outline"
              onclick={() => config.zones.splice(zi, 1)}
              title="The BMC keeps the last speed it was given for this zone">Remove zone</button
            >
          </header>

          <div class="grid grid-cols-[repeat(auto-fill,minmax(12rem,1fr))] gap-x-4">
            <fieldset class="fieldset">
              <legend class="fieldset-legend">Name</legend>
              <input class="input w-full" bind:value={zone.name} />
            </fieldset>
            <fieldset class="fieldset">
              <legend class="fieldset-legend">IPMI zone</legend>
              <select class="select w-full" bind:value={zone.id}>
                <option value={0}>0 — CPU fans (FAN1…)</option>
                <option value={1}>1 — Peripheral fans (FANA…)</option>
                {#each [2, 3] as id (id)}<option value={id}>{id}</option>{/each}
              </select>
            </fieldset>
            <fieldset class="fieldset">
              <legend class="fieldset-legend">Minimum speed (%)</legend>
              <input class="input w-full" type="number" min="0" max="100" bind:value={zone.min_duty} />
            </fieldset>
            <fieldset class="fieldset">
              <legend class="fieldset-legend">Maximum speed (%)</legend>
              <input class="input w-full" type="number" min="0" max="100" bind:value={zone.max_duty} />
            </fieldset>
            <fieldset class="fieldset" title="How far the temperature must drop before the fans slow down">
              <legend class="fieldset-legend">Slow down after a drop of (°C)</legend>
              <input
                class="input w-full"
                type="number"
                min="0"
                max="20"
                step="0.5"
                bind:value={zone.hysteresis}
              />
            </fieldset>
          </div>

          {#if zone.id === 0 || zone.id === 1}
            <CalibrationPanel {zone} />
          {/if}

          {#each zone.rules as rule, ri (ri)}
            <div class="grid gap-3 border-t border-base-300 pt-4">
              <div class="flex flex-wrap items-end justify-between gap-4">
                <fieldset class="fieldset max-w-105 flex-[1_1_260px]">
                  <legend class="fieldset-legend">Temperature source</legend>
                  <select class="select w-full" bind:value={rule.source}>
                    {#if !sources.some((s) => s.id === rule.source)}
                      <option value={rule.source}>{rule.source} (not found now)</option>
                    {/if}
                    {#each sources as s, i (i)}
                      <option value={s.id}>{s.label}{s.celsius === null ? '' : ` — ${s.celsius}°C`}</option>
                    {/each}
                  </select>
                </fieldset>
                <button class="btn btn-ghost btn-sm mb-1" onclick={() => zone.rules.splice(ri, 1)}
                  >Remove source</button
                >
              </div>
              <div class="join" role="group" aria-label="How this source drives the fans">
                <button
                  class="btn join-item btn-sm {rule.target ? '' : 'btn-primary'}"
                  aria-pressed={!rule.target}
                  onclick={() => (rule.target = null)}>Follow the curve</button
                >
                <button
                  class="btn join-item btn-sm {rule.target ? 'btn-primary' : ''}"
                  aria-pressed={!!rule.target}
                  onclick={() =>
                    (rule.target = rule.target ?? {
                      celsius: defaultTarget(rule.source),
                      reactivity: 'normal',
                    })}>Hold a temperature</button
                >
              </div>
              {#if rule.target}
                <div class="flex flex-wrap items-end gap-x-5 gap-y-2">
                  <fieldset class="fieldset w-48">
                    <legend class="fieldset-legend">Target (°C)</legend>
                    <input
                      class="input w-full"
                      type="number"
                      min="10"
                      max="100"
                      step="1"
                      bind:value={rule.target.celsius}
                    />
                  </fieldset>
                  <fieldset class="fieldset w-60">
                    <legend class="fieldset-legend">Reactivity</legend>
                    <select class="select w-full" bind:value={rule.target.reactivity}>
                      <option value="calm">Calm: slow, quiet changes</option>
                      <option value="normal">Normal</option>
                      <option value="reactive">Reactive: follows quickly</option>
                    </select>
                  </fieldset>
                  <p class="flex-[1_1_20rem] pb-2 text-sm text-base-content/70">
                    Fans run at the lowest speed that keeps this source at {rule.target.celsius}°C. The curve
                    is the starting point and the safety net: from {rule.target.celsius +
                      safetyMargin(rule.source)}°C it applies again.
                  </p>
                </div>
              {/if}
              <CurveEditor
                bind:points={rule.curve}
                probe={probeForSource(rule.source, diskKind)}
                current={currentTemp(rule.source)}
                minDuty={zone.min_duty}
                maxDuty={zone.max_duty}
                label={zone.name}
                target={rule.target?.celsius ?? null}
                safetyFrom={rule.target ? rule.target.celsius + safetyMargin(rule.source) : null}
                actualDuty={liveDuty(zone.id, ri)}
              />
            </div>
          {:else}
            <div role="status" class="alert alert-soft">
              No source: this zone is not controlled. Add a source to drive it from a temperature.
            </div>
          {/each}
          <div><button class="btn btn-sm" onclick={() => addRule(zone)}>Add a source</button></div>
        </div>
      </section>
    {/each}
    <div><button class="btn btn-dash btn-primary" onclick={addZone}>+ Add a zone</button></div>
  {:else}
    <p class="flex items-center gap-2 text-base-content/60">
      <span class="loading loading-spinner loading-sm"></span> Loading…
    </p>
  {/if}
</div>
