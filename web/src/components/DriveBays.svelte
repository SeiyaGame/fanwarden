<script lang="ts">
  // Disks drawn as the front of a NAS: one bay per drive, filled to its temperature.
  import { band, thermalColor } from '../lib/thermal'
  import type { Disk } from '../lib/types'

  let { disks }: { disks: Disk[] } = $props()

  const FLOOR = 15
  const CEIL = 60
  const fill = (t: number | null) =>
    t === null ? 0 : Math.max(4, Math.min(100, ((t - FLOOR) / (CEIL - FLOOR)) * 100))

  const kindLabel = { hdd: 'HDD', ssd: 'SSD', nvme: 'NVMe' } as const
</script>

{#if disks.length === 0}
  <p class="soft">
    No disk found. Check that smartctl and lsblk are installed, or the exclusion filter in Settings.
  </p>
{:else}
  <ul class="bays">
    {#each disks as disk (disk.name)}
      <li
        class="bay"
        class:asleep={disk.state === 'standby'}
        class:error={disk.state === 'error'}
        title="{disk.model} — serial {disk.serial}{disk.error ? `\n${disk.error}` : ''}"
      >
        <div class="slot" aria-hidden="true">
          <div
            class="level"
            style:height="{fill(disk.temperature)}%"
            style:background={thermalColor(disk.temperature, disk.kind)}
          ></div>
          <span class="led" data-band={band(disk.temperature, disk.kind) ?? 'none'}></span>
        </div>
        <div class="reading figure">
          {#if disk.state === 'standby'}
            <span class="soft">asleep</span>
          {:else if disk.state === 'error'}
            <span class="fault">no data</span>
          {:else}
            {disk.temperature}°
          {/if}
        </div>
        <div class="name">{disk.name}</div>
        <div class="kind soft">{kindLabel[disk.kind]}</div>
      </li>
    {/each}
  </ul>
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
    display: grid;
    justify-items: center;
    gap: 2px;
    min-width: 0;
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

  .led {
    position: absolute;
    top: 6px;
    left: 6px;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--mild);
    box-shadow: 0 0 0 2px var(--panel-sunk);
  }

  .asleep .led {
    background: var(--rule);
  }

  .error .led {
    background: var(--critical);
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
</style>
