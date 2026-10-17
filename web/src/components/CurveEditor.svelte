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

  const DOMAINS: Record<Probe, [number, number]> = {
    cpu: [20, 100],
    hdd: [15, 60],
    ssd: [20, 80],
    nvme: [20, 90],
    board: [15, 80],
  }

  const W = 640
  const H = 280
  const PAD = { left: 40, right: 14, top: 14, bottom: 30 }

  const domain = $derived.by((): [number, number] => {
    const [lo, hi] = DOMAINS[probe]
    const temps = points.map((p) => p[0])
    return [
      Math.min(lo, ...temps.map((t) => Math.floor(t / 5) * 5 - 5)),
      Math.max(hi, ...temps.map((t) => t + 5)),
    ]
  })

  const x = (t: number) => PAD.left + ((t - domain[0]) / (domain[1] - domain[0])) * (W - PAD.left - PAD.right)
  const y = (d: number) => H - PAD.bottom - (d / 100) * (H - PAD.top - PAD.bottom)
  const tempOf = (px: number) =>
    domain[0] + ((px - PAD.left) / (W - PAD.left - PAD.right)) * (domain[1] - domain[0])
  const dutyOf = (py: number) => ((H - PAD.bottom - py) / (H - PAD.top - PAD.bottom)) * 100

  const xTicks = $derived.by(() => {
    const ticks: number[] = []
    const step = domain[1] - domain[0] > 60 ? 10 : 5
    for (let t = Math.ceil(domain[0] / step) * step; t <= domain[1]; t += step) ticks.push(t)
    return ticks
  })

  const path = $derived.by(() => {
    if (points.length === 0) return ''
    const coords: [number, number][] = [
      [domain[0], points[0][1]],
      ...points,
      [domain[1], points[points.length - 1][1]],
    ]
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

    {#each [0, 25, 50, 75, 100] as d (d)}
      <line class="grid" x1={PAD.left} x2={W - PAD.right} y1={y(d)} y2={y(d)} />
      <text class="tick" x={PAD.left - 6} y={y(d) + 4} text-anchor="end">{d}%</text>
    {/each}
    {#each xTicks as t (t)}
      <text class="tick" x={x(t)} y={H - PAD.bottom + 18} text-anchor="middle">{t}°</text>
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
