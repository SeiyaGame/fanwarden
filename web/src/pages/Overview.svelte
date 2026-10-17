<script lang="ts">
  import DriveBays from '../components/DriveBays.svelte'
  import Rotor from '../components/Rotor.svelte'
  import ZonePanel from '../components/ZonePanel.svelte'
  import { live } from '../lib/live.svelte'
  import { thermalColor } from '../lib/thermal'

  const s = $derived(live.snapshot)
  const boardTemps = $derived(s?.sensors.ipmi.filter((t) => t.celsius !== null) ?? [])
</script>

{#if !s || s.ts === 0}
  <p class="soft">Waiting for the first reading…</p>
{:else}
  <div class="overview">
    {#if s.alerts.length}
      <section class="alerts" aria-label="Active alerts">
        {#each s.alerts as alert (alert.key)}
          <p data-severity={alert.severity}>
            <strong>{alert.severity === 'critical' ? 'Critical' : 'Warning'}</strong>
            {alert.message}
          </p>
        {/each}
      </section>
    {/if}

    {#if s.bmc_error}
      <p class="bmc-error panel">
        <strong>BMC unreachable.</strong>
        {s.bmc_error}. Check the IPMI settings, or that ipmitool can reach the board.
      </p>
    {/if}

    <div class="zones">
      {#each s.zones as zone (zone.id)}
        <ZonePanel {zone} fans={s.fans} />
      {/each}
    </div>

    <section class="panel" aria-labelledby="disks-title">
      <header class="row">
        <h2 id="disks-title">Disks</h2>
        {#if !s.sensors.disks_fresh && !s.simulate}
          <span class="stale">Readings are out of date</span>
        {/if}
      </header>
      <DriveBays disks={s.sensors.disks} />
    </section>

    <div class="pair">
      <section class="panel" aria-labelledby="fans-title">
        <h2 id="fans-title">Fans</h2>
        <ul class="fans">
          {#each s.fans as fan (fan.name)}
            <li class:empty={fan.rpm === null}>
              <Rotor rpm={fan.rpm} size={34} />
              <span class="name">{fan.name}</span>
              <span class="figure">{fan.rpm === null ? 'not connected' : `${fan.rpm} RPM`}</span>
            </li>
          {:else}
            <li class="soft">The BMC reports no fan.</li>
          {/each}
        </ul>
      </section>

      <section class="panel" aria-labelledby="sensors-title">
        <h2 id="sensors-title">Temperatures</h2>
        <dl class="temps">
          <div>
            <dt>CPU package</dt>
            <dd class="figure">
              <span class="dot" style:background={thermalColor(s.sensors.cpu, 'cpu')}></span>
              {s.sensors.cpu === null ? '—' : `${s.sensors.cpu.toFixed(1)}°C`}
            </dd>
          </div>
          {#each boardTemps as t (t.name)}
            <div>
              <dt>{t.name}</dt>
              <dd class="figure">
                <span
                  class="dot"
                  style:background={thermalColor(
                    t.celsius,
                    t.name.toLowerCase().includes('cpu') ? 'cpu' : 'board',
                  )}
                ></span>
                {t.celsius}°C
              </dd>
            </div>
          {/each}
        </dl>
      </section>
    </div>
  </div>
{/if}

<style>
  .overview {
    display: grid;
    gap: var(--gutter);
  }

  .zones,
  .pair {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(100%, 340px), 1fr));
    gap: var(--gutter);
  }

  section.panel {
    display: grid;
    gap: 1rem;
    align-content: start;
  }

  .row {
    display: flex;
    gap: 1rem;
    align-items: baseline;
    flex-wrap: wrap;
  }

  .stale {
    color: var(--hot);
    font-size: var(--step--1);
  }

  .alerts {
    display: grid;
    gap: 0.5rem;
  }

  .alerts p,
  .bmc-error {
    border-left: 4px solid var(--warm);
    background: var(--panel);
    padding: 0.7rem 1rem;
    border-radius: var(--radius-s);
  }

  .alerts p[data-severity='critical'],
  .bmc-error {
    border-left-color: var(--critical);
  }

  .fans {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: 0.75rem 1rem;
  }

  .fans li {
    display: grid;
    grid-template-columns: auto 1fr;
    grid-template-rows: auto auto;
    column-gap: 0.6rem;
    align-items: center;
  }

  .fans li > :global(svg) {
    grid-row: span 2;
  }

  .fans .name {
    font-weight: 500;
  }

  .fans .figure {
    font-weight: 500;
    color: var(--ink-soft);
  }

  .fans li.empty {
    opacity: 0.6;
  }

  .temps {
    margin: 0;
    display: grid;
    gap: 0.4rem;
  }

  .temps div {
    display: flex;
    justify-content: space-between;
    gap: 1rem;
    border-bottom: 1px solid var(--rule);
    padding-bottom: 0.35rem;
  }

  .temps dt {
    color: var(--ink-soft);
  }

  .temps dd {
    margin: 0;
    display: flex;
    align-items: center;
    gap: 0.45rem;
  }

  .dot {
    width: 0.55rem;
    height: 0.55rem;
    border-radius: 50%;
  }
</style>
