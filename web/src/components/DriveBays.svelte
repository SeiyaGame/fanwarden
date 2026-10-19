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
  <p class="soft">
    No disk found. Check that smartctl and lsblk are installed, or the exclusion filter in Settings.
  </p>
{:else}
  <ul class="bays">
    {#each disks as disk (disk.name)}
      <li>
        <button
          class="bay"
          class:asleep={disk.state === 'standby'}
          class:selected={disk.serial === selectedSerial}
          aria-expanded={disk.serial === selectedSerial}
          aria-label="{disk.name}: {HEALTH_TEXT[healthLevel(disk)]}"
          onclick={() => (selectedSerial = selectedSerial === disk.serial ? null : disk.serial)}
        >
          <span class="slot" aria-hidden="true">
            <span
              class="level"
              style:height="{fill(disk.temperature)}%"
              style:background={thermalColor(disk.temperature, disk.kind)}
            ></span>
            <span class="led" data-health={disk.state === 'error' ? 'failing' : healthLevel(disk)}></span>
          </span>
          <span class="reading figure">
            {#if disk.state === 'standby'}
              <span class="soft">asleep</span>
            {:else if disk.state === 'error'}
              <span class="fault">no data</span>
            {:else}
              {disk.temperature}°
            {/if}
          </span>
          <span class="name">{disk.name}</span>
          <span class="kind soft">{kindLabel[disk.kind]}</span>
        </button>
      </li>
    {/each}
  </ul>

  {#if selected}
    {@const level = healthLevel(selected)}
    <div class="details" data-health={level}>
      <p class="verdict"><span class="led" data-health={level}></span>{HEALTH_TEXT[level]}</p>
      <dl>
        <div>
          <dt>Disk</dt>
          <dd>{selected.name} — {selected.model || kindLabel[selected.kind]}</dd>
        </div>
        <div>
          <dt>Serial</dt>
          <dd>{selected.serial}</dd>
        </div>
        {#if selected.error}<div>
            <dt>Error</dt>
            <dd class="fault">{selected.error}</dd>
          </div>{/if}
        {#if selected.health?.passed !== null && selected.health?.passed !== undefined}
          <div>
            <dt>SMART self-assessment</dt>
            <dd class:fault={!selected.health.passed}>{selected.health.passed ? 'Passed' : 'FAILED'}</dd>
          </div>
        {/if}
        {#if selected.health?.power_on_hours != null}
          <div>
            <dt>Powered on</dt>
            <dd>
              {powerOnText(selected.health.power_on_hours)}
              <span class="soft">({selected.health.power_on_hours.toLocaleString()} h)</span>
            </dd>
          </div>
        {/if}
        {#each counters(selected) as row (row.label)}
          <div>
            <dt>{row.label}</dt>
            <dd class="figure" class:warn={row.bad}>{row.value}</dd>
          </div>
        {/each}
      </dl>
    </div>
  {/if}
{/if}

<style>
  .bays {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(64px, 1fr));
    gap: 10px;
  }

  .bay {
    all: unset;
    box-sizing: border-box;
    width: 100%;
    display: grid;
    justify-items: center;
    gap: 2px;
    min-width: 0;
    padding: 4px 0;
    border-radius: var(--radius-s);
    cursor: pointer;
  }

  .bay:hover .slot,
  .bay.selected .slot {
    border-color: var(--ink-soft);
  }

  .bay:focus-visible {
    outline: 2px solid var(--focus);
    outline-offset: 2px;
  }

  .slot {
    position: relative;
    width: 100%;
    max-width: 58px;
    height: 104px;
    border-radius: 3px;
    background:
      repeating-linear-gradient(
        0deg,
        transparent 0 7px,
        color-mix(in srgb, var(--rule) 55%, transparent) 7px 8px
      ),
      var(--panel-sunk);
    border: 1px solid var(--rule);
    overflow: hidden;
  }

  .level {
    position: absolute;
    inset: auto 0 0 0;
    opacity: 0.85;
    transition: height 0.6s ease;
  }

  .slot .led {
    position: absolute;
    top: 6px;
    left: 6px;
    box-shadow: 0 0 0 2px var(--panel-sunk);
  }

  .led {
    display: inline-block;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--rule);
  }

  .led[data-health='ok'] {
    background: var(--mild);
  }

  .led[data-health='warning'] {
    background: var(--warm);
  }

  .led[data-health='failing'] {
    background: var(--critical);
  }

  .asleep .level {
    opacity: 0.4;
  }

  .reading {
    font-size: var(--step-1);
    margin-top: 4px;
  }

  .reading span {
    font-family: var(--font);
    font-weight: 400;
    font-size: var(--step--1);
  }

  .fault {
    color: var(--critical);
  }

  .name {
    font-size: var(--step--1);
    font-weight: 500;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .kind {
    font-size: 0.75rem;
  }

  .details {
    border-left: 4px solid var(--mild);
    background: var(--panel-sunk);
    border-radius: var(--radius-s);
    padding: 0.8rem 1rem;
    display: grid;
    gap: 0.6rem;
  }

  .details[data-health='warning'] {
    border-left-color: var(--warm);
  }

  .details[data-health='failing'] {
    border-left-color: var(--critical);
  }

  .details[data-health='unknown'] {
    border-left-color: var(--rule);
  }

  .verdict {
    margin: 0;
    font-weight: 600;
    display: flex;
    align-items: center;
    gap: 0.5rem;
  }

  .verdict .led {
    width: 8px;
    height: 8px;
  }

  dl {
    margin: 0;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(100%, 240px), 1fr));
    gap: 0.4rem 1.5rem;
  }

  dl div {
    display: flex;
    justify-content: space-between;
    gap: 1rem;
    border-bottom: 1px solid var(--rule);
    padding-bottom: 0.3rem;
    min-width: 0;
  }

  dt {
    color: var(--ink-soft);
    white-space: nowrap;
  }

  dd {
    margin: 0;
    text-align: right;
    overflow-wrap: anywhere;
  }

  dd.warn {
    color: var(--hot);
    font-weight: 600;
  }
</style>
