<script lang="ts">
  // Calibration of a zone: sweep from 100% to 0%, then suggest a minimum speed from where the
  // fans stopped. The speeds are kept as the reference that detects fans slowing down over time.
  import { api } from '../lib/api'
  import { live } from '../lib/live.svelte'
  import type { Zone } from '../lib/types'

  let { zone }: { zone: Zone } = $props()

  const status = $derived(live.snapshot?.calibration ?? null)
  const reference = $derived(live.snapshot?.references[zone.id] ?? null)
  const running = $derived(status?.state === 'running' ? status : null)
  const runningHere = $derived(running?.zone === zone.id)
  const failedHere = $derived(status?.state === 'failed' && status.zone === zone.id ? status.reason : null)
  const saved = $derived(live.snapshot?.zones.some((z) => z.id === zone.id) ?? false)

  let error = $state<string | null>(null)

  async function act(call: () => Promise<unknown>) {
    error = null
    try {
      await call()
    } catch (e) {
      error = e instanceof Error ? e.message : String(e)
    }
  }

  // Chart of the reference: speed by duty, one line per fan.
  const W = 320
  const H = 120
  const maxRpm = $derived(Math.max(1, ...(reference?.fans.flatMap((f) => f.points.map((p) => p[1])) ?? [1])))
  const x = (duty: number) => (duty / 100) * W
  const y = (rpm: number) => H - (rpm / maxRpm) * H
  const path = (points: [number, number][]) =>
    points.map(([d, r], i) => `${i ? 'L' : 'M'}${x(d).toFixed(1)},${y(r).toFixed(1)}`).join(' ')
  /** Fan names at the 100% end of their line, pushed apart when the lines end close together. */
  const labels = $derived.by(() => {
    const ends = (reference?.fans ?? [])
      .filter((f) => f.points.length)
      .map((f) => ({ name: f.name, y: y(f.points[0][1]) + 4 }))
      .sort((a, b) => a.y - b.y)
    for (let i = 1; i < ends.length; i++) ends[i].y = Math.max(ends[i].y, ends[i - 1].y + 11)
    return ends
  })
</script>

<div class="calibration">
  <div class="head">
    <h3>Calibration</h3>
    <span class="soft">
      {reference ? `Last run ${new Date(reference.ts * 1000).toLocaleString()}` : 'Never calibrated'}
    </span>
  </div>

  {#if runningHere && running}
    <div class="progress">
      <progress max={running.steps} value={running.step}></progress>
      <span class="figure">{running.duty}%</span>
      <span class="soft">step {running.step} of {running.steps}</span>
      <button class="quiet" onclick={() => act(() => api.cancelCalibration(zone.id))}>Cancel</button>
    </div>
    <p class="soft">
      The fans slow down step by step and stop at the end. If a temperature reaches its alert limit, the
      calibration stops and the fans go back to full speed.
    </p>
  {:else}
    <p class="soft">
      Sweeps this zone from 100% down to 0% (about 4 minutes) to find where the fans stop, and keeps their
      speeds to warn you when a fan gets slower over time. It stops and goes back to full speed if a
      temperature reaches its alert limit.
    </p>
    <div>
      <button
        class="quiet"
        disabled={!!running || !saved}
        title={!saved ? 'Save the zone first' : running ? 'Another zone is being calibrated' : undefined}
        onclick={() => act(() => api.startCalibration(zone.id))}>Calibrate</button
      >
    </div>
  {/if}

  {#if failedHere}<p class="fault">Calibration stopped: {failedHere}.</p>{/if}
  {#if error}<p class="fault">{error}</p>{/if}

  {#if reference && !runningHere}
    {#if reference.bmc_takeover !== null}
      <div class="note">
        <p>
          The BMC raised the fans to full speed at {reference.bmc_takeover}%: a fan went under one of its
          lower RPM thresholds, so the sweep stopped there. Check them with
          <code>ipmitool sensor</code>; if they are high, lower them and calibrate again:
        </p>
        <pre>{reference.fans
            .map((f) => `ipmitool sensor thresh ${f.name} lower 100 200 300`)
            .join('\n')}</pre>
      </div>
    {/if}
    {#if reference.suggested_min_duty !== null}
      <div class="suggestion">
        <p>
          {#if reference.stall_duty !== null}
            Fans stop at <strong>{reference.stall_duty}%</strong>: minimum speed
            <strong>{reference.suggested_min_duty}%</strong> suggested, a margin above the lowest speed that still
            turns them
          {:else}
            Minimum speed <strong>{reference.suggested_min_duty}%</strong> suggested, a margin above where the BMC
            takes over
          {/if}
          <span class="soft">(now {zone.min_duty}%)</span>.
        </p>
        <button
          disabled={zone.min_duty === reference.suggested_min_duty}
          onclick={() => (zone.min_duty = reference.suggested_min_duty ?? zone.min_duty)}
          >Use {reference.suggested_min_duty}%</button
        >
      </div>
    {:else if reference.bmc_takeover === null}
      <p>No fan stopped, even at 0%: the minimum speed can be as low as you like.</p>
    {/if}

    <figure>
      <svg
        viewBox="-34 -8 {W + 70} {H + 30}"
        role="img"
        aria-label="Fan speed by duty during the calibration"
      >
        <line class="axis" x1="0" y1={H} x2={W} y2={H} />
        <line class="axis" x1="0" y1="0" x2="0" y2={H} />
        <text class="tick" x="-6" y="4" text-anchor="end">{maxRpm}</text>
        <text class="tick" x="-6" y={H} text-anchor="end">0</text>
        {#each [0, 25, 50, 75, 100] as d (d)}
          <text class="tick" x={x(d)} y={H + 16} text-anchor="middle">{d}%</text>
        {/each}
        <line class="min" x1={x(zone.min_duty)} y1="0" x2={x(zone.min_duty)} y2={H} />
        {#each reference.fans as fan (fan.name)}
          <path class="fan" d={path(fan.points)} />
        {/each}
        {#each labels as label (label.name)}
          <text class="label" x={W + 4} y={label.y}>{label.name}</text>
        {/each}
      </svg>
      <figcaption class="soft">RPM by duty. Dashed line: the minimum speed of the zone.</figcaption>
    </figure>
  {/if}
</div>

<style>
  .calibration {
    display: grid;
    gap: 0.6rem;
    border-top: 1px solid var(--rule);
    padding-top: 1.1rem;
  }

  .head {
    display: flex;
    gap: 1rem;
    align-items: baseline;
    flex-wrap: wrap;
  }

  h3 {
    font-size: var(--step-0);
  }

  .head span,
  p.soft,
  figcaption {
    font-size: var(--step--1);
  }

  p {
    margin: 0;
  }

  .progress {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 0.75rem;
  }

  progress {
    flex: 1 1 200px;
    max-width: 420px;
    accent-color: var(--control);
  }

  .suggestion {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 0.5rem 1rem;
  }

  .note {
    border-left: 4px solid var(--warm);
    padding-left: 0.75rem;
    display: grid;
    gap: 0.4rem;
  }

  .note pre {
    margin: 0;
    font-size: var(--step--1);
    overflow-x: auto;
  }

  .fault {
    color: var(--critical);
  }

  figure {
    margin: 0;
    max-width: 520px;
  }

  svg {
    width: 100%;
    height: auto;
    overflow: visible;
  }

  .axis {
    stroke: var(--rule);
  }

  .tick,
  .label {
    font-size: 10px;
    fill: var(--ink-soft);
  }

  .min {
    stroke: var(--ink-soft);
    stroke-dasharray: 3 3;
  }

  .fan {
    fill: none;
    stroke: var(--ink);
    stroke-width: 1.5;
    stroke-linejoin: round;
  }
</style>
