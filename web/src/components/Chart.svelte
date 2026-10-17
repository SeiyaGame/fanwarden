<script lang="ts">
  // Time series chart (uPlot): one unit per chart, legend with values under the cursor.
  import uPlot from 'uplot'
  import 'uplot/dist/uPlot.min.css'
  import { onDestroy } from 'svelte'
  import type { HistoryData } from '../lib/types'

  let { data, label, unit }: { data: HistoryData; label: (key: string) => string; unit: string } = $props()

  // Categorical palette in fixed order (validated for colour-blind separation); past 8 series, neutral grey.
  const LIGHT = ['#2a78d6', '#eb6834', '#1baf7a', '#eda100', '#e87ba4', '#008300', '#6250d6', '#e34948']
  const DARK = ['#3987e5', '#d95926', '#199e70', '#c98500', '#d55181', '#008300', '#9085e9', '#e66767']
  const dark = window.matchMedia('(prefers-color-scheme: dark)').matches
  const color = (i: number) => (dark ? DARK : LIGHT)[i] ?? (dark ? '#6b7a85' : '#9aa5ad')
  const ink = dark ? '#8c9ba6' : '#55636e'
  const grid = dark ? '#2b3943' : '#dbe1e5'

  let host: HTMLDivElement
  let plot: uPlot | null = null
  let observer: ResizeObserver | null = null

  function build() {
    plot?.destroy()
    const axis = {
      stroke: ink,
      grid: { stroke: grid, width: 1 },
      ticks: { show: false },
      font: '12px Barlow',
    }
    const opts: uPlot.Options = {
      width: host.clientWidth,
      height: 260,
      cursor: { drag: { x: true, y: false } },
      scales: { x: { time: true } },
      legend: { live: true },
      axes: [axis, { ...axis, size: 54, values: (_u, values) => values.map((v) => `${v}${unit}`) }],
      series: [
        {},
        ...data.series.map((s, i) => ({
          label: label(s.key),
          stroke: color(i),
          width: 2,
          spanGaps: false,
          value: (_u: uPlot, v: number | null) => (v === null ? '—' : `${v}${unit}`),
          points: { show: false },
        })),
      ],
    }
    plot = new uPlot(opts, [data.timestamps, ...data.series.map((s) => s.values)] as uPlot.AlignedData, host)
  }

  $effect(() => {
    // Rebuild when the data changes (series may appear or disappear).
    void data
    build()
  })

  $effect(() => {
    observer = new ResizeObserver(() => plot?.setSize({ width: host.clientWidth, height: 260 }))
    observer.observe(host)
    return () => observer?.disconnect()
  })

  onDestroy(() => plot?.destroy())
</script>

<div class="chart" bind:this={host}></div>

<style>
  .chart {
    width: 100%;
    min-height: 300px;
  }

  .chart :global(.u-legend) {
    font-family: var(--font);
    font-size: var(--step--1);
    color: var(--ink);
    text-align: left;
  }

  .chart :global(.u-legend .u-value) {
    font-family: var(--font-figures);
    font-variant-numeric: tabular-nums;
  }

  .chart :global(.u-select) {
    background: color-mix(in srgb, var(--focus) 15%, transparent);
  }
</style>
