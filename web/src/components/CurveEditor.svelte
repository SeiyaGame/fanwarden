<script lang="ts">
  // Temperature → fan speed curve. Drag points, click the plot to add one, Delete to remove the selected one.
  import { curveProblems, dutyAt, insertPoint } from '../lib/curve'
  import { thermalColor, type Probe } from '../lib/thermal'
  import type { CurvePoint } from '../lib/types'

  let {
    points = $bindable(),
    probe,
    current = null,
    minDuty = 0,
    maxDuty = 100,
    label,
    target = null,
    safetyFrom = null,
    actualDuty = null,
  }: {
    points: CurvePoint[]
    probe: Probe
    current?: number | null
    minDuty?: number
    maxDuty?: number
    label: string
    /** Target mode: temperature held by the regulator. */
    target?: number | null
    /** Target mode: temperature from which the curve is enforced. */
    safetyFrom?: number | null
    /** Target mode: duty chosen by the regulator right now. */
    actualDuty?: number | null
  } = $props()

  const clipId = `safety-${Math.random().toString(36).slice(2, 9)}`

  /** Range where precision matters, per kind of sensor: it gets most of the width. */
  const USEFUL: Record<Probe, [number, number]> = {
    cpu: [20, 90],
    hdd: [25, 55],
    ssd: [25, 70],
    nvme: [30, 80],
    board: [20, 70],
  }

  const W = 640
  const H = 320
  const PAD = { left: 40, right: 14, top: 14, bottom: 30 }
  const PLOT_W = W - PAD.left - PAD.right
  /** Share of the width given to each side outside the useful range. */
  const SQUEEZED = 0.08

  const domain = $derived.by((): [number, number] => {
    const [lo, hi] = USEFUL[probe]
    const temps = points.map((p) => p[0])
    return [
      Math.min(lo, ...temps.map((t) => Math.floor(t / 5) * 5 - 5)),
      Math.max(hi, ...temps.map((t) => Math.ceil(t / 5) * 5 + 5)),
    ]
  })

  // Piecewise-linear temperature axis: [start, low] and [high, end] are squeezed on the sides.
  const low = $derived(Math.max(domain[0], USEFUL[probe][0]))
  const high = $derived(Math.min(domain[1], USEFUL[probe][1]))
  const stops = $derived.by(() => {
    const before = low > domain[0] ? SQUEEZED : 0
    const after = high < domain[1] ? SQUEEZED : 0
    return { temps: [domain[0], low, high, domain[1]], shares: [0, before, 1 - after, 1] }
  })

  function lerpStops(value: number, from: number[], to: number[]): number {
    for (let i = 1; i < from.length; i++) {
      if (value <= from[i] || i === from.length - 1) {
        const span = from[i] - from[i - 1]
        return span === 0 ? to[i] : to[i - 1] + ((value - from[i - 1]) / span) * (to[i] - to[i - 1])
      }
    }
    return to[to.length - 1]
  }

  const x = (t: number) => PAD.left + lerpStops(t, stops.temps, stops.shares) * PLOT_W
  const tempOf = (px: number) => lerpStops((px - PAD.left) / PLOT_W, stops.shares, stops.temps)
  const y = (d: number) => H - PAD.bottom - (d / 100) * (H - PAD.top - PAD.bottom)
  const dutyOf = (py: number) => ((H - PAD.bottom - py) / (H - PAD.top - PAD.bottom)) * 100

  /** Vertical grid every 5°C over the useful range; labels thinned out when the range is wide. */
  const xGrid = $derived.by(() => {
    const lines: { t: number; label: boolean }[] = []
    const labelStep = high - low > 50 ? 10 : 5
    for (let t = Math.ceil(low / 5) * 5; t <= high; t += 5) lines.push({ t, label: t % labelStep === 0 })
    if (low > domain[0]) lines.unshift({ t: domain[0], label: true })
    if (high < domain[1]) lines.push({ t: domain[1], label: true })
    return lines
  })

  const path = $derived.by(() => {
    if (points.length === 0) return ''
    const coords: [number, number][] = [
      [domain[0], points[0][1]],
      ...points,
      [domain[1], points[points.length - 1][1]],
    ]
    for (const t of [low, high]) if (!points.some((p) => p[0] === t)) coords.push([t, dutyAt(points, t)])
    coords.sort((a, b) => a[0] - b[0])
    return coords.map(([t, d], i) => `${i ? 'L' : 'M'}${x(t).toFixed(1)},${y(d).toFixed(1)}`).join(' ')
  })

  const curveDuty = $derived(
    current === null ? null : Math.min(maxDuty, Math.max(minDuty, dutyAt(points, current))),
  )
  const currentDuty = $derived(target !== null && actualDuty !== null ? actualDuty : curveDuty)
  const problems = $derived(curveProblems(points))

  let svg: SVGSVGElement
  let selected = $state<number | null>(null)
  let dragging = $state<number | null>(null)

  function toLocal(event: PointerEvent): [number, number] {
    const pt = new DOMPoint(event.clientX, event.clientY).matrixTransform(svg.getScreenCTM()!.inverse())
    return [pt.x, pt.y]
  }

  const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v))

  function move(index: number, temp: number, duty: number) {
    const lo = index > 0 ? points[index - 1][0] + 1 : -20
    const hi = index < points.length - 1 ? points[index + 1][0] - 1 : 150
    points[index] = [clamp(Math.round(temp), lo, hi), clamp(Math.round(duty), 0, 100)]
  }

  function onPointDown(event: PointerEvent, index: number) {
    event.stopPropagation()
    selected = index
    dragging = index
    svg.setPointerCapture(event.pointerId)
  }

  function onPointerMove(event: PointerEvent) {
    if (dragging === null) return
    const [px, py] = toLocal(event)
    move(dragging, tempOf(px), dutyOf(py))
  }

  function onPlotDown(event: PointerEvent) {
    if (event.button !== 0) return
    const [px, py] = toLocal(event)
    if (px < PAD.left || py > H - PAD.bottom) return
    const point: CurvePoint = [Math.round(tempOf(px)), clamp(Math.round(dutyOf(py)), 0, 100)]
    points = insertPoint(points, point)
    selected = points.findIndex((p) => p[0] === point[0])
  }

  function remove(index: number) {
    if (points.length <= 1) return
    points = points.filter((_, i) => i !== index)
    selected = null
  }

  function onKey(event: KeyboardEvent, index: number) {
    const [t, d] = points[index]
    const step = event.shiftKey ? 5 : 1
    const moves: Record<string, [number, number]> = {
      ArrowLeft: [t - step, d],
      ArrowRight: [t + step, d],
      ArrowUp: [t, d + step],
      ArrowDown: [t, d - step],
    }
    if (event.key in moves) {
      event.preventDefault()
      move(index, ...moves[event.key])
    } else if (event.key === 'Delete' || event.key === 'Backspace') {
      event.preventDefault()
      remove(index)
    }
  }
</script>

<div class="grid gap-3">
  <svg
    class="h-auto w-full touch-none select-none"
    bind:this={svg}
    viewBox="0 0 {W} {H}"
    role="application"
    aria-label="{label}: fan speed by temperature"
    onpointermove={onPointerMove}
    onpointerup={() => (dragging = null)}
    onpointercancel={() => (dragging = null)}
  >
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <rect
      class="cursor-crosshair fill-base-200"
      x={PAD.left}
      y={PAD.top}
      width={W - PAD.left - PAD.right}
      height={H - PAD.top - PAD.bottom}
      onpointerdown={onPlotDown}
    />

    <g class="pointer-events-none">
      {#if low > domain[0]}
        <rect
          class="fill-base-content/6"
          x={PAD.left}
          y={PAD.top}
          width={x(low) - PAD.left}
          height={y(0) - PAD.top}
        />
      {/if}
      {#if high < domain[1]}
        <rect
          class="fill-base-content/6"
          x={x(high)}
          y={PAD.top}
          width={W - PAD.right - x(high)}
          height={y(0) - PAD.top}
        />
      {/if}
      {#each [0, 10, 20, 30, 40, 50, 60, 70, 80, 90, 100] as d (d)}
        <line
          class="stroke-base-300 {d % 25 === 0 ? '' : 'opacity-45'}"
          x1={PAD.left}
          x2={W - PAD.right}
          y1={y(d)}
          y2={y(d)}
        />
      {/each}
      <g class="fill-base-content/60 font-figures text-xs">
        {#each [0, 25, 50, 75, 100] as d (d)}
          <text x={PAD.left - 6} y={y(d) + 4} text-anchor="end">{d}%</text>
        {/each}
        {#each xGrid as g (g.t)}
          {#if g.label}
            <text x={x(g.t)} y={H - PAD.bottom + 18} text-anchor="middle">{g.t}°</text>
          {/if}
        {/each}
      </g>
      {#each xGrid as g (g.t)}
        <line
          class="stroke-base-300 {g.label ? '' : 'opacity-45'}"
          x1={x(g.t)}
          x2={x(g.t)}
          y1={PAD.top}
          y2={H - PAD.bottom}
        />
      {/each}

      {#if minDuty > 0}
        <rect
          class="fill-base-content/9"
          x={PAD.left}
          y={y(minDuty)}
          width={W - PAD.left - PAD.right}
          height={y(0) - y(minDuty)}
        />
      {/if}
      {#if maxDuty < 100}
        <rect
          class="fill-base-content/9"
          x={PAD.left}
          y={y(100)}
          width={W - PAD.left - PAD.right}
          height={y(maxDuty) - y(100)}
        />
      {/if}

      {#if target !== null && safetyFrom !== null}
        <!-- Target mode: the curve only applies from the safety threshold, the rest is faded. -->
        <clipPath id={clipId}>
          <rect x={x(safetyFrom)} y="0" width={W} height={H} />
        </clipPath>
        <path
          class="fill-none stroke-base-content stroke-[2.5] opacity-25 [stroke-dasharray:6_5] [stroke-linejoin:round]"
          d={path}
        />
        <path
          class="fill-none stroke-base-content stroke-[2.5] [stroke-linejoin:round]"
          d={path}
          clip-path="url(#{clipId})"
        />
        <line
          class="stroke-hot stroke-[1.5] [stroke-dasharray:3_3]"
          x1={x(safetyFrom)}
          x2={x(safetyFrom)}
          y1={PAD.top}
          y2={H - PAD.bottom}
        />
        <text class="fill-hot text-xs font-semibold" x={x(safetyFrom) + 6} y={H - PAD.bottom - 8}
          >Safety curve from {safetyFrom}°</text
        >
        <line class="stroke-mild stroke-3" x1={x(target)} x2={x(target)} y1={PAD.top} y2={H - PAD.bottom} />
        <text
          class="fill-mild text-xs font-semibold"
          x={x(target) - 6}
          y={H - PAD.bottom - 8}
          text-anchor="end">Target {target}°</text
        >
      {:else}
        <path class="fill-none stroke-base-content stroke-[2.5] [stroke-linejoin:round]" d={path} />
      {/if}

      {#if current !== null && currentDuty !== null}
        <line
          class="stroke-base-content/60 [stroke-dasharray:4_4]"
          x1={x(current)}
          x2={x(current)}
          y1={PAD.top}
          y2={H - PAD.bottom}
        />
        <circle
          class="stroke-base-100 stroke-2"
          cx={x(current)}
          cy={y(currentDuty)}
          r="6"
          fill={thermalColor(current, probe)}
        />
        <text class="fill-base-content text-xs font-semibold" x={x(current) + 8} y={PAD.top + 12}
          >Now {current}° → {Math.round(currentDuty)}%</text
        >
      {/if}
    </g>

    {#each points as [t, d], i (i)}
      <g
        class="group cursor-grab outline-none"
        transform="translate({x(t)},{y(d)})"
        tabindex="0"
        role="slider"
        aria-label="Point {i + 1}: {t}°C, {d}%"
        aria-valuenow={d}
        aria-valuemin={0}
        aria-valuemax={100}
        onpointerdown={(e) => onPointDown(e, i)}
        onkeydown={(e) => onKey(e, i)}
        onfocus={() => (selected = i)}
      >
        <circle
          class="fill-transparent group-focus-visible:stroke-primary group-focus-visible:stroke-2"
          r="16"
        />
        <circle
          class="stroke-base-content stroke-[2.5] group-focus-visible:fill-base-content {selected === i
            ? 'fill-base-content'
            : 'fill-base-100'}"
          r="6.5"
        />
      </g>
    {/each}
  </svg>

  {#each problems as problem (problem)}
    <div role="alert" class="alert alert-error alert-soft py-2 text-sm">{problem}</div>
  {/each}
  <p class="flex min-h-9 flex-wrap items-center gap-x-4 gap-y-2 text-sm">
    {#if selected !== null && points[selected]}
      {@const index = selected}
      <span class="font-semibold">Point {index + 1}</span>
      <label class="input input-sm w-28"
        ><input
          type="number"
          aria-label="Temperature"
          value={points[index][0]}
          onchange={(e) => move(index, Number(e.currentTarget.value), points[index][1])}
        /><span class="label">°C</span></label
      >
      <label class="input input-sm w-28"
        ><input
          type="number"
          aria-label="Fan speed"
          value={points[index][1]}
          min="0"
          max="100"
          onchange={(e) => move(index, points[index][0], Number(e.currentTarget.value))}
        /><span class="label">%</span></label
      >
      <button class="btn btn-ghost btn-sm" disabled={points.length <= 1} onclick={() => remove(index)}
        >Remove point</button
      >
    {:else}
      <span class="text-base-content/60"
        >Click the plot to add a point. Drag a point, or select it and use the arrow keys.</span
      >
    {/if}
  </p>
</div>
