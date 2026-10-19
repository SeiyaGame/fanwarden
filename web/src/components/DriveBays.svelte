<script lang="ts">
  // Disks drawn as the front of a NAS: one bay per drive, filled to its temperature. The LED shows
  // the SMART health; a click on a bay shows the details.
  import { HEALTH_TEXT, healthLevel, powerOnText } from '../lib/health'
  import { thermalColor } from '../lib/thermal'
  import type { Disk } from '../lib/types'

  let { disks }: { disks: Disk[] } = $props()

  const FLOOR = 15
  const CEIL = 60
  const fill = (t: number | null) =>
    t === null ? 0 : Math.max(4, Math.min(100, ((t - FLOOR) / (CEIL - FLOOR)) * 100))

  const kindLabel = { hdd: 'HDD', ssd: 'SSD', nvme: 'NVMe' } as const
  const STATUS = {
    ok: 'status-success',
    warning: 'status-warning',
    failing: 'status-error',
    unknown: '',
  } as const
  const BOX = {
    ok: 'border-success/40 bg-success/5',
    warning: 'border-warning/60 bg-warning/10',
    failing: 'border-error/60 bg-error/10',
    unknown: 'border-base-300 bg-base-200',
  } as const

  let selectedSerial = $state<string | null>(null)
  const selected = $derived(disks.find((d) => d.serial === selectedSerial) ?? null)

  /** Counters worth a line in the details: only the ones this drive reports. */
  function counters(disk: Disk): { label: string; value: string; bad: boolean }[] {
    const h = disk.health
    if (!h) return []
    const rows: { label: string; value: number | null; bad: boolean; suffix?: string }[] = [
      { label: 'Reallocated sectors', value: h.reallocated, bad: (h.reallocated ?? 0) > 0 },
      { label: 'Pending sectors', value: h.pending, bad: (h.pending ?? 0) > 0 },
      { label: 'Media errors', value: h.media_errors, bad: (h.media_errors ?? 0) > 0 },
      { label: 'CRC errors (cable)', value: h.crc_errors, bad: false },
      { label: 'Wear', value: h.percentage_used, bad: (h.percentage_used ?? 0) >= 90, suffix: '% used' },
    ]
    return rows
      .filter((r) => r.value !== null)
      .map((r) => ({ label: r.label, value: `${r.value}${r.suffix ?? ''}`, bad: r.bad }))
  }
</script>

{#if disks.length === 0}
  <p class="text-base-content/60">
    No disk found. Check that smartctl and lsblk are installed, or the exclusion filter in Settings.
  </p>
{:else}
  <ul class="grid grid-cols-[repeat(auto-fill,minmax(4.25rem,1fr))] gap-2.5">
    {#each disks as disk (disk.name)}
      {@const level = disk.state === 'error' ? 'failing' : healthLevel(disk)}
      <li>
        <button
          class="group grid w-full cursor-pointer justify-items-center gap-0.5 rounded-box py-1 focus-visible:outline-2 focus-visible:outline-primary"
          aria-expanded={disk.serial === selectedSerial}
          aria-label="{disk.name}: {HEALTH_TEXT[healthLevel(disk)]}"
          onclick={() => (selectedSerial = selectedSerial === disk.serial ? null : disk.serial)}
        >
          <span
            class="relative h-26 w-full max-w-14 overflow-hidden rounded border bg-base-200 transition-colors group-hover:border-base-content/50 {disk.serial ===
            selectedSerial
              ? 'border-base-content/60'
              : 'border-base-300'}"
            aria-hidden="true"
          >
            <span
              class="absolute inset-x-0 bottom-0 transition-[height] duration-500 {disk.state === 'standby'
                ? 'opacity-40'
                : 'opacity-85'}"
              style:height="{fill(disk.temperature)}%"
              style:background-color={thermalColor(disk.temperature, disk.kind)}
            ></span>
            <span class="status absolute top-1.5 left-1.5 {STATUS[level]}"></span>
          </span>
          <span class="mt-1 font-figures text-lg font-semibold tabular-nums">
            {#if disk.state === 'standby'}
              <span class="font-sans text-xs font-normal text-base-content/60">asleep</span>
            {:else if disk.state === 'error'}
              <span class="font-sans text-xs font-normal text-error">no data</span>
            {:else}
              {disk.temperature}°
            {/if}
          </span>
          <span class="max-w-full truncate text-sm font-medium">{disk.name}</span>
          <span class="text-xs text-base-content/60">{kindLabel[disk.kind]}</span>
        </button>
      </li>
    {/each}
  </ul>

  {#if selected}
    {@const level = healthLevel(selected)}
    <div class="mt-2 grid gap-3 rounded-box border p-4 {BOX[level]}">
      <p class="flex items-center gap-2 font-semibold">
        <span class="status {STATUS[level]}"></span>{HEALTH_TEXT[level]}
      </p>
      <dl class="grid grid-cols-[repeat(auto-fill,minmax(11rem,1fr))] gap-x-6 gap-y-3">
        <div class="col-span-full sm:col-span-2">
          <dt class="text-xs text-base-content/60">Disk</dt>
          <dd class="font-medium">{selected.name} — {selected.model || kindLabel[selected.kind]}</dd>
        </div>
        <div>
          <dt class="text-xs text-base-content/60">Serial</dt>
          <dd class="font-mono text-sm">{selected.serial}</dd>
        </div>
        {#if selected.health?.passed !== null && selected.health?.passed !== undefined}
          <div>
            <dt class="text-xs text-base-content/60">SMART self-assessment</dt>
            <dd class={selected.health.passed ? 'font-medium' : 'font-semibold text-error'}>
              {selected.health.passed ? 'Passed' : 'FAILED'}
            </dd>
          </div>
        {/if}
        {#if selected.health?.power_on_hours != null}
          <div>
            <dt class="text-xs text-base-content/60">Powered on</dt>
            <dd class="font-medium">
              {powerOnText(selected.health.power_on_hours)}
              <span class="text-sm font-normal text-base-content/60"
                >({selected.health.power_on_hours.toLocaleString()} h)</span
              >
            </dd>
          </div>
        {/if}
        {#each counters(selected) as row (row.label)}
          <div>
            <dt class="text-xs text-base-content/60">{row.label}</dt>
            <dd class="font-figures text-lg font-semibold tabular-nums {row.bad ? 'text-warning' : ''}">
              {row.value}
            </dd>
          </div>
        {/each}
        {#if selected.error}
          <div class="col-span-full">
            <dt class="text-xs text-base-content/60">Error</dt>
            <dd class="text-error">{selected.error}</dd>
          </div>
        {/if}
      </dl>
    </div>
  {/if}
{/if}
