<script lang="ts">
  import DriveBays from '../components/DriveBays.svelte'
  import Rotor from '../components/Rotor.svelte'
  import ZonePanel from '../components/ZonePanel.svelte'
  import { live } from '../lib/live.svelte'
  import { thermalColor } from '../lib/thermal'

  const s = $derived(live.snapshot)
  const boardTemps = $derived(s?.sensors.ipmi.filter((t) => t.celsius !== null) ?? [])
  const hottestDisk = $derived(
    s?.sensors.disks
      .filter((d) => d.temperature !== null)
      .reduce<(typeof s.sensors.disks)[number] | null>(
        (hot, d) => (hot === null || (d.temperature ?? 0) > (hot.temperature ?? 0) ? d : hot),
        null,
      ) ?? null,
  )
  const spinning = $derived(s?.fans.filter((f) => (f.rpm ?? 0) > 0).length ?? 0)
  const connected = $derived(s?.fans.filter((f) => f.rpm !== null).length ?? 0)
</script>

{#if !s || s.ts === 0}
  <div class="flex items-center gap-3 text-base-content/60">
    <span class="loading loading-spinner"></span> Waiting for the first reading…
  </div>
{:else}
  <div class="grid gap-6">
    {#if s.alerts.length}
      <section class="grid gap-2" aria-label="Active alerts">
        {#each s.alerts as alert (alert.key)}
          <div
            role="alert"
            class="alert {alert.severity === 'critical' ? 'alert-error' : 'alert-warning'} alert-soft"
          >
            <strong>{alert.severity === 'critical' ? 'Critical' : 'Warning'}</strong>
            <span>{alert.message}</span>
          </div>
        {/each}
      </section>
    {/if}

    {#if s.bmc_error}
      <div role="alert" class="alert alert-error">
        <span
          ><strong>BMC unreachable.</strong>
          {s.bmc_error}. Check the IPMI settings, or that ipmitool can reach the board.</span
        >
      </div>
    {/if}

    <div class="stats stats-vertical w-full border border-base-300 bg-base-100 sm:stats-horizontal">
      <div class="stat">
        <div class="stat-title">CPU</div>
        <div class="stat-value font-figures" style:color={thermalColor(s.sensors.cpu, 'cpu')}>
          {s.sensors.cpu === null ? '—' : `${s.sensors.cpu.toFixed(0)}°C`}
        </div>
        <div class="stat-desc">package temperature</div>
      </div>
      <div class="stat">
        <div class="stat-title">Hottest disk</div>
        <div
          class="stat-value font-figures"
          style:color={hottestDisk ? thermalColor(hottestDisk.temperature, hottestDisk.kind) : undefined}
        >
          {hottestDisk ? `${hottestDisk.temperature}°C` : '—'}
        </div>
        <div class="stat-desc">{hottestDisk ? hottestDisk.name : 'no disk reading'}</div>
      </div>
      <div class="stat">
        <div class="stat-title">Fans</div>
        <div class="stat-value font-figures">
          {spinning}<span class="text-base-content/40">/{connected}</span>
        </div>
        <div class="stat-desc">spinning</div>
      </div>
      {#if s.power_watts !== null}
        <div class="stat">
          <div class="stat-title">Power draw</div>
          <div class="stat-value font-figures">{Math.round(s.power_watts)} W</div>
          <div class="stat-desc">whole server</div>
        </div>
      {/if}
    </div>

    <div class="grid gap-6 md:grid-cols-2">
      {#each s.zones as zone (zone.id)}
        <ZonePanel {zone} fans={s.fans} />
      {/each}
    </div>

    <section class="card border border-base-300 bg-base-100" aria-labelledby="disks-title">
      <div class="card-body">
        <div class="flex flex-wrap items-center gap-3">
          <h2 id="disks-title" class="card-title">Disks</h2>
          {#if !s.sensors.disks_fresh && !s.simulate}
            <span class="badge badge-warning badge-soft">Readings are out of date</span>
          {/if}
        </div>
        <DriveBays disks={s.sensors.disks} />
      </div>
    </section>

    <div class="grid gap-6 md:grid-cols-2">
      <section class="card border border-base-300 bg-base-100" aria-labelledby="fans-title">
        <div class="card-body">
          <h2 id="fans-title" class="card-title">Fans</h2>
          <ul class="list">
            {#each s.fans as fan, i (i)}
              <li class="list-row items-center py-2 {fan.rpm === null ? 'opacity-50' : ''}">
                <Rotor rpm={fan.rpm} size={30} />
                <span class="font-medium">{fan.name}</span>
                <span class="font-figures font-semibold tabular-nums"
                  >{fan.rpm === null ? 'not connected' : `${fan.rpm} RPM`}</span
                >
              </li>
            {:else}
              <li class="text-base-content/60">The BMC reports no fan.</li>
            {/each}
          </ul>
        </div>
      </section>

      <section class="card border border-base-300 bg-base-100" aria-labelledby="sensors-title">
        <div class="card-body">
          <h2 id="sensors-title" class="card-title">Temperatures</h2>
          <table class="table">
            <tbody>
              <tr>
                <th class="font-normal text-base-content/70">CPU package</th>
                <td class="text-right font-figures font-semibold tabular-nums">
                  <span class="status mr-2" style:background-color={thermalColor(s.sensors.cpu, 'cpu')}
                  ></span>
                  {s.sensors.cpu === null ? '—' : `${s.sensors.cpu.toFixed(1)}°C`}
                </td>
              </tr>
              {#each boardTemps as t, i (i)}
                <tr>
                  <th class="font-normal text-base-content/70">{t.name}</th>
                  <td class="text-right font-figures font-semibold tabular-nums">
                    <span
                      class="status mr-2"
                      style:background-color={thermalColor(
                        t.celsius,
                        t.name.toLowerCase().includes('cpu') ? 'cpu' : 'board',
                      )}
                    ></span>
                    {t.celsius}°C
                  </td>
                </tr>
              {/each}
            </tbody>
          </table>
        </div>
      </section>
    </div>
  </div>
{/if}
