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

<div class="grid gap-3 border-t border-base-300 pt-4">
  <div class="flex flex-wrap items-baseline gap-x-4 gap-y-1">
    <h3 class="font-semibold">Calibration</h3>
    <span class="text-sm text-base-content/60">
      {reference ? `Last run ${new Date(reference.ts * 1000).toLocaleString()}` : 'Never calibrated'}
    </span>
  </div>

  {#if runningHere && running}
    <div class="flex flex-wrap items-center gap-3">
      <progress
        class="progress progress-warning max-w-105 flex-[1_1_200px]"
        max={running.steps}
        value={running.step}
      ></progress>
      <span class="font-figures font-semibold tabular-nums">{running.duty}%</span>
      <span class="text-sm text-base-content/60">step {running.step} of {running.steps}</span>
      <button class="btn btn-sm" onclick={() => act(() => api.cancelCalibration(zone.id))}>Cancel</button>
    </div>
    <p class="text-sm text-base-content/70">
      The fans slow down step by step and stop at the end. If a temperature reaches its alert limit, the
      calibration stops and the fans go back to full speed.
    </p>
  {:else}
    <p class="text-sm text-base-content/70">
      Sweeps this zone from 100% down to 0% (about 4 minutes) to find where the fans stop, and keeps their
      speeds to warn you when a fan gets slower over time. It stops and goes back to full speed if a
      temperature reaches its alert limit.
    </p>
    <div>
      <button
        class="btn btn-sm"
        disabled={!!running || !saved}
        title={!saved ? 'Save the zone first' : running ? 'Another zone is being calibrated' : undefined}
        onclick={() => act(() => api.startCalibration(zone.id))}>Calibrate</button
      >
    </div>
  {/if}

  {#if failedHere}<div role="alert" class="alert alert-error alert-soft">
      Calibration stopped: {failedHere}.
    </div>{/if}
  {#if error}<div role="alert" class="alert alert-error alert-soft">{error}</div>{/if}

  {#if reference && !runningHere}
    {#if reference.bmc_takeover !== null}
      <div role="alert" class="grid gap-2 rounded-box border border-warning/60 bg-warning/10 p-4">
        <p>
          The BMC raised the fans to full speed at {reference.bmc_takeover}%: a fan went under one of its
          lower RPM thresholds, so the sweep stopped there. Check them with
          <code class="font-mono">ipmitool sensor</code>; if they are high, lower them and calibrate again:
        </p>
        <pre class="overflow-x-auto rounded bg-base-100 p-2 font-mono text-xs">{reference.fans
            .map((f) => `ipmitool sensor thresh ${f.name} lower 100 200 300`)
            .join('\n')}</pre>
      </div>
    {/if}
    {#if reference.suggested_min_duty !== null}
      <div role="status" class="alert alert-info alert-soft flex flex-wrap">
        <p class="flex-1">
          {#if reference.stall_duty !== null}
            Fans stop at <strong>{reference.stall_duty}%</strong>: minimum speed
            <strong>{reference.suggested_min_duty}%</strong> suggested, a margin above the lowest speed that still
            turns them
          {:else}
            Minimum speed <strong>{reference.suggested_min_duty}%</strong> suggested, a margin above where the BMC
            takes over
          {/if}
          <span class="text-base-content/60">(now {zone.min_duty}%)</span>.
        </p>
        <button
          class="btn btn-sm btn-primary"
          disabled={zone.min_duty === reference.suggested_min_duty}
          onclick={() => (zone.min_duty = reference.suggested_min_duty ?? zone.min_duty)}
          >Use {reference.suggested_min_duty}%</button
        >
      </div>
    {:else if reference.bmc_takeover === null}
      <p>No fan stopped, even at 0%: the minimum speed can be as low as you like.</p>
    {/if}

    <div class="grid max-w-130 gap-1">
      <svg
        class="h-auto w-full overflow-visible text-[10px]"
        viewBox="-34 -8 {W + 70} {H + 30}"
        role="img"
        aria-label="Fan speed by duty during the calibration"
      >
        <line class="stroke-base-300" x1="0" y1={H} x2={W} y2={H} />
        <line class="stroke-base-300" x1="0" y1="0" x2="0" y2={H} />
        <g class="fill-base-content/60">
          <text x="-6" y="4" text-anchor="end">{maxRpm}</text>
          <text x="-6" y={H} text-anchor="end">0</text>
          {#each [0, 25, 50, 75, 100] as d (d)}
            <text x={x(d)} y={H + 16} text-anchor="middle">{d}%</text>
          {/each}
          {#each labels as label (label.name)}
            <text x={W + 4} y={label.y}>{label.name}</text>
          {/each}
        </g>
        <line
          class="stroke-base-content/60 [stroke-dasharray:3_3]"
          x1={x(zone.min_duty)}
          y1="0"
          x2={x(zone.min_duty)}
          y2={H}
        />
        {#each reference.fans as fan (fan.name)}
          <path
            class="fill-none stroke-base-content stroke-[1.5] [stroke-linejoin:round]"
            d={path(fan.points)}
          />
        {/each}
      </svg>
      <p class="text-sm text-base-content/60">RPM by duty. Dashed line: the minimum speed of the zone.</p>
    </div>
  {/if}
</div>
