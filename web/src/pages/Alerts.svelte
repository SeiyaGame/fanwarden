<script lang="ts">
  import OptionalNumber from '../components/OptionalNumber.svelte'
  import { api } from '../lib/api'
  import { configStore } from '../lib/configStore.svelte'
  import { live } from '../lib/live.svelte'
  import type { NotificationChannel } from '../lib/types'

  const config = $derived(configStore.draft)
  const active = $derived(live.snapshot?.alerts ?? [])

  const KINDS: { type: NotificationChannel['type']; label: string }[] = [
    { type: 'discord', label: 'Discord' },
    { type: 'ntfy', label: 'ntfy' },
    { type: 'gotify', label: 'Gotify' },
    { type: 'webhook', label: 'Webhook (JSON)' },
  ]
  let newKind = $state<NotificationChannel['type']>('discord')
  let testResult = $state<Record<number, string>>({})

  function blank(type: NotificationChannel['type']): NotificationChannel {
    switch (type) {
      case 'discord':
        return { type, enabled: true, url: '' }
      case 'ntfy':
        return { type, enabled: true, server: 'https://ntfy.sh', topic: '', token: '' }
      case 'gotify':
        return { type, enabled: true, server: '', token: '' }
      case 'webhook':
        return { type, enabled: true, url: '' }
    }
  }

  async function test(channel: NotificationChannel, index: number) {
    testResult[index] = 'Sending…'
    try {
      const savedCount = configStore.saved?.notifications.length ?? 0
      await api.testNotification(
        $state.snapshot(channel) as NotificationChannel,
        index < savedCount ? index : null,
      )
      testResult[index] = 'Sent. Check that it arrived.'
    } catch (e) {
      testResult[index] = `Failed: ${e instanceof Error ? e.message : e}`
    }
  }

  const since = (ts: number) => new Date(ts * 1000).toLocaleString()
</script>

<div class="page">
  <h1>Alerts</h1>

  <section class="panel" aria-labelledby="active-title">
    <h2 id="active-title">Active now</h2>
    {#each active as alert (alert.key)}
      <p class="alert" data-severity={alert.severity}>
        {alert.message} <span class="soft">since {since(alert.since)}</span>
      </p>
    {:else}
      <p class="soft">Nothing to report.</p>
    {/each}
  </section>

  {#if config}
    <section class="panel" aria-labelledby="rules-title">
      <h2 id="rules-title">When to alert</h2>
      <div class="grid-fields">
        <OptionalNumber bind:value={config.alerts.cpu_high} label="CPU above" unit="°C" fallback={85} />
        <OptionalNumber
          bind:value={config.alerts.disk_high}
          label="HDD or SSD above"
          unit="°C"
          fallback={45}
        />
        <OptionalNumber bind:value={config.alerts.nvme_high} label="NVMe above" unit="°C" fallback={70} />
        <OptionalNumber bind:value={config.alerts.disk_low} label="Disk below" unit="°C" fallback={10} />
        <OptionalNumber bind:value={config.alerts.fan_min_rpm} label="Fan below" unit="RPM" fallback={150} />
        <label class="field"
          >BMC unreachable after (failed commands)
          <input type="number" min="0" bind:value={config.alerts.ipmi_failures} /></label
        >
        <label class="field"
          >Repeat an alert every (minutes, 0 = never)
          <input type="number" min="0" bind:value={config.alerts.repeat_minutes} /></label
        >
        <label class="check"
          ><input type="checkbox" bind:checked={config.alerts.notify_resolved} /> Notify when an alert clears</label
        >
      </div>
      <p class="soft">
        A zone running at failsafe speed and a disk that cannot be read always raise an alert.
      </p>
    </section>

    <section class="panel" aria-labelledby="channels-title">
      <h2 id="channels-title">Where to send them</h2>
      {#each config.notifications as channel, i (i)}
        <div class="channel">
          <div class="channel-head">
            <label class="check"
              ><input type="checkbox" bind:checked={channel.enabled} />
              <strong>{KINDS.find((k) => k.type === channel.type)?.label}</strong></label
            >
            <div class="actions">
              <button class="quiet" onclick={() => test(channel, i)}>Send a test</button>
              <button class="danger" onclick={() => config.notifications.splice(i, 1)}>Remove</button>
            </div>
          </div>
          <div class="grid-fields">
            {#if channel.type === 'discord' || channel.type === 'webhook'}
              <label class="field wide"
                >URL <input bind:value={channel.url} placeholder="https://…" autocomplete="off" /></label
              >
            {:else if channel.type === 'ntfy'}
              <label class="field">Server <input bind:value={channel.server} /></label>
              <label class="field">Topic <input bind:value={channel.topic} /></label>
              <label class="field"
                >Access token (optional) <input bind:value={channel.token} autocomplete="off" /></label
              >
            {:else if channel.type === 'gotify'}
              <label class="field">Server <input bind:value={channel.server} placeholder="https://…" /></label
              >
              <label class="field"
                >Application token <input bind:value={channel.token} autocomplete="off" /></label
              >
            {/if}
          </div>
          {#if testResult[i]}<p class="soft">{testResult[i]}</p>{/if}
        </div>
      {:else}
        <p class="soft">No destination yet: alerts only appear here and in the journal.</p>
      {/each}
      <div class="add">
        <label class="visually-hidden" for="new-kind">Destination type</label>
        <select id="new-kind" bind:value={newKind}>
          {#each KINDS as k (k.type)}<option value={k.type}>{k.label}</option>{/each}
        </select>
        <button class="quiet" onclick={() => config.notifications.push(blank(newKind))}
          >Add destination</button
        >
      </div>
    </section>
  {/if}
</div>

<style>
  .page {
    display: grid;
    gap: var(--gutter);
  }

  section.panel {
    display: grid;
    gap: 1rem;
  }

  .alert {
    border-left: 4px solid var(--warm);
    padding-left: 0.75rem;
  }

  .alert[data-severity='critical'] {
    border-left-color: var(--critical);
  }

  .channel {
    display: grid;
    gap: 0.75rem;
    border-top: 1px solid var(--rule);
    padding-top: 1rem;
  }

  .channel-head {
    display: flex;
    justify-content: space-between;
    flex-wrap: wrap;
    gap: 0.75rem;
  }

  .actions,
  .add {
    display: flex;
    flex-wrap: wrap;
    gap: 0.5rem;
  }

  .wide {
    grid-column: 1 / -1;
  }
</style>
