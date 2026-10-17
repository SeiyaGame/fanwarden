<script lang="ts">
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

<div class="page">
  <header class="intro">
    <h1>Fan curves</h1>
    <p class="soft">
      Each zone follows the hottest of its sources: every source has its own curve, and the zone runs at the
      highest speed they ask for. A zone whose source cannot be read runs at the failsafe speed.
    </p>
  </header>

  {#if config}
    {#each config.zones as zone, zi (zi)}
      <section class="panel zone" aria-labelledby="zone-title-{zi}">
        <header class="zone-head">
          <h2 id="zone-title-{zi}">{zone.name || `Zone ${zone.id}`}</h2>
          <button
            class="danger"
            onclick={() => config.zones.splice(zi, 1)}
            title="The BMC keeps the last speed it was given for this zone">Remove zone</button
          >
        </header>

        <div class="grid-fields">
          <label class="field">Name <input bind:value={zone.name} /></label>
          <label class="field"
            >IPMI zone
            <select bind:value={zone.id}>
              <option value={0}>0 — CPU fans (FAN1…)</option>
              <option value={1}>1 — Peripheral fans (FANA…)</option>
              {#each [2, 3] as id (id)}<option value={id}>{id}</option>{/each}
            </select>
          </label>
          <label class="field"
            >Minimum speed (%) <input type="number" min="0" max="100" bind:value={zone.min_duty} /></label
          >
          <label class="field"
            >Maximum speed (%) <input type="number" min="0" max="100" bind:value={zone.max_duty} /></label
          >
          <label class="field" title="How far the temperature must drop before the fans slow down"
            >Slow down after a drop of (°C)
            <input type="number" min="0" max="20" step="0.5" bind:value={zone.hysteresis} /></label
          >
        </div>

        {#each zone.rules as rule, ri (ri)}
          <div class="rule">
            <div class="rule-head">
              <label class="field"
                >Temperature source
                <select bind:value={rule.source}>
                  {#if !sources.some((s) => s.id === rule.source)}
                    <option value={rule.source}>{rule.source} (not found now)</option>
                  {/if}
                  {#each sources as s, i (i)}
                    <option value={s.id}>{s.label}{s.celsius === null ? '' : ` — ${s.celsius}°C`}</option>
                  {/each}
                </select>
              </label>
              <button class="quiet" onclick={() => zone.rules.splice(ri, 1)}>Remove source</button>
            </div>
            <div class="mode" role="group" aria-label="How this source drives the fans">
              <button
                class:quiet={!!rule.target}
                aria-pressed={!rule.target}
                onclick={() => (rule.target = null)}>Follow the curve</button
              >
              <button
                class:quiet={!rule.target}
                aria-pressed={!!rule.target}
                onclick={() =>
                  (rule.target = rule.target ?? {
                    celsius: defaultTarget(rule.source),
                    reactivity: 'normal',
                  })}>Hold a temperature</button
              >
            </div>
            {#if rule.target}
              <div class="target-fields">
                <label class="field"
                  >Target (°C)
                  <input type="number" min="10" max="100" step="1" bind:value={rule.target.celsius} /></label
                >
                <label class="field"
                  >Reactivity
                  <select bind:value={rule.target.reactivity}>
                    <option value="calm">Calm: slow, quiet changes</option>
                    <option value="normal">Normal</option>
                    <option value="reactive">Reactive: follows quickly</option>
                  </select>
                </label>
                <p class="soft">
                  Fans run at the lowest speed that keeps this source at {rule.target.celsius}°C. The curve is
                  the starting point and the safety net: from {rule.target.celsius +
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
          <p class="soft">
            No source: this zone is not controlled. Add a source to drive it from a temperature.
          </p>
        {/each}
        <div><button class="quiet" onclick={() => addRule(zone)}>Add a source</button></div>
      </section>
    {/each}
    <div><button class="quiet" onclick={addZone}>Add a zone</button></div>
  {:else}
    <p class="soft">Loading…</p>
  {/if}
</div>

<style>
  .page {
    display: grid;
    gap: var(--gutter);
  }

  .intro {
    display: grid;
    gap: 0.5rem;
    max-width: 70ch;
  }

  .zone {
    display: grid;
    gap: 1.25rem;
  }

  .zone-head,
  .rule-head {
    display: flex;
    justify-content: space-between;
    align-items: end;
    gap: 1rem;
    flex-wrap: wrap;
  }

  .rule {
    display: grid;
    gap: 0.8rem;
    border-top: 1px solid var(--rule);
    padding-top: 1.1rem;
  }

  .mode {
    display: flex;
    flex-wrap: wrap;
    gap: 0.35rem;
  }

  .target-fields {
    display: flex;
    flex-wrap: wrap;
    align-items: end;
    gap: 0.8rem 1.2rem;
  }

  .target-fields .field {
    width: 12rem;
  }

  .target-fields p {
    flex: 1 1 20rem;
    font-size: var(--step--1);
  }

  .rule-head .field {
    flex: 1 1 260px;
    max-width: 420px;
  }
</style>
