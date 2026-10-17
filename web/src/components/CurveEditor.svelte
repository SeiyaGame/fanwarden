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
  }: {
    points: CurvePoint[]
    probe: Probe
    current?: number | null
    minDuty?: number
    maxDuty?: number
    label: string
  } = $props()

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

  const currentDuty = $derived(
    current === null ? null : Math.min(maxDuty, Math.max(minDuty, dutyAt(points, current))),
  )
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

<figure class="editor">
  <svg
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
      class="plot"
      x={PAD.left}
      y={PAD.top}
      width={W - PAD.left - PAD.right}
      height={H - PAD.top - PAD.bottom}
      onpointerdown={onPlotDown}
    />

    {#if low > domain[0]}
      <rect class="squeezed" x={PAD.left} y={PAD.top} width={x(low) - PAD.left} height={y(0) - PAD.top} />
    {/if}
    {#if high < domain[1]}
      <rect
        class="squeezed"
        x={x(high)}
        y={PAD.top}
        width={W - PAD.right - x(high)}
        height={y(0) - PAD.top}
      />
    {/if}
    {#each [0, 10, 20, 30, 40, 50, 60, 70, 80, 90, 100] as d (d)}
      <line class="grid" class:major={d % 25 === 0} x1={PAD.left} x2={W - PAD.right} y1={y(d)} y2={y(d)} />
    {/each}
    {#each [0, 25, 50, 75, 100] as d (d)}
      <text class="tick" x={PAD.left - 6} y={y(d) + 4} text-anchor="end">{d}%</text>
    {/each}
    {#each xGrid as g (g.t)}
      <line class="grid" class:major={g.label} x1={x(g.t)} x2={x(g.t)} y1={PAD.top} y2={H - PAD.bottom} />
      {#if g.label}
        <text class="tick" x={x(g.t)} y={H - PAD.bottom + 18} text-anchor="middle">{g.t}°</text>
      {/if}
    {/each}

    {#if minDuty > 0}
      <rect
        class="limit"
        x={PAD.left}
        y={y(minDuty)}
        width={W - PAD.left - PAD.right}
        height={y(0) - y(minDuty)}
      />
    {/if}
    {#if maxDuty < 100}
      <rect
        class="limit"
        x={PAD.left}
        y={y(100)}
        width={W - PAD.left - PAD.right}
        height={y(maxDuty) - y(100)}
      />
    {/if}

    <path class="curve" d={path} />

    {#if current !== null && currentDuty !== null}
      <line class="now" x1={x(current)} x2={x(current)} y1={PAD.top} y2={H - PAD.bottom} />
      <circle class="now-dot" cx={x(current)} cy={y(currentDuty)} r="6" fill={thermalColor(current, probe)} />
      <text class="now-label" x={x(current) + 8} y={PAD.top + 12}
        >Now {current}° → {Math.round(currentDuty)}%</text
      >
    {/if}

    {#each points as [t, d], i (i)}
      <g
        class="point"
        class:selected={selected === i}
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
        <circle class="hit" r="16" />
        <circle class="dot" r="6.5" />
      </g>
    {/each}
  </svg>

  {#each problems as problem (problem)}
    <p class="problem">{problem}</p>
  {/each}
  <figcaption>
    {#if selected !== null && points[selected]}
      {@const index = selected}
      <span>Point {index + 1}</span>
      <label class="inline"
        ><input
          type="number"
          value={points[index][0]}
          onchange={(e) => move(index, Number(e.currentTarget.value), points[index][1])}
        />°C</label
      >
      <label class="inline"
        ><input
          type="number"
          value={points[index][1]}
          min="0"
          max="100"
          onchange={(e) => move(index, points[index][0], Number(e.currentTarget.value))}
        />%</label
      >
      <button class="quiet" disabled={points.length <= 1} onclick={() => remove(index)}>Remove point</button>
    {:else}
      <span class="soft"
        >Click the plot to add a point. Drag a point, or select it and use the arrow keys.</span
      >
    {/if}
  </figcaption>
</figure>

<style>
  .editor {
    margin: 0;
    display: grid;
    gap: 0.6rem;
  }

  svg {
    width: 100%;
    height: auto;
    touch-action: none;
    user-select: none;
  }

  .plot {
    fill: var(--panel-sunk);
    cursor: crosshair;
  }

  .grid {
    stroke: var(--rule);
    stroke-width: 1;
    opacity: 0.45;
    pointer-events: none;
  }

  .grid.major {
    opacity: 1;
  }

  .squeezed {
    fill: var(--ink-soft);
    opacity: 0.06;
    pointer-events: none;
  }

  .tick {
    fill: var(--ink-soft);
    font-size: 12px;
    font-family: var(--font-figures);
  }

  .limit {
    fill: var(--ink-soft);
    opacity: 0.09;
    pointer-events: none;
  }

  .curve {
    fill: none;
    stroke: var(--ink);
    stroke-width: 2.5;
    stroke-linejoin: round;
    pointer-events: none;
  }

  .now {
    stroke: var(--ink-soft);
    stroke-dasharray: 4 4;
    pointer-events: none;
  }

  .now-dot {
    stroke: var(--panel);
    stroke-width: 2;
    pointer-events: none;
  }

  .now-label {
    fill: var(--ink);
    font-size: 12px;
    font-weight: 600;
    pointer-events: none;
  }

  .point {
    cursor: grab;
    outline: none;
  }

  .point .hit {
    fill: transparent;
  }

  .point .dot {
    fill: var(--panel);
    stroke: var(--ink);
    stroke-width: 2.5;
  }

  .point.selected .dot,
  .point:focus-visible .dot {
    fill: var(--ink);
  }

  .point:focus-visible .hit {
    stroke: var(--focus);
    stroke-width: 2;
  }

  figcaption {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 0.6rem 1rem;
    min-height: 2.3rem;
    font-size: var(--step--1);
  }

  .inline {
    display: flex;
    align-items: center;
    gap: 0.3rem;
  }

  .inline input {
    width: 4.5rem;
  }

  .problem {
    color: var(--critical);
    font-size: var(--step--1);
  }
</style>
