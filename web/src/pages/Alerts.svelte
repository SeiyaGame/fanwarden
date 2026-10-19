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

<div class="grid gap-6">
  <h1 class="text-2xl font-semibold">Alerts</h1>

  <section class="card border border-base-300 bg-base-100" aria-labelledby="active-title">
    <div class="card-body">
      <h2 id="active-title" class="card-title">Active now</h2>
      {#each active as alert (alert.key)}
        <div
          role="alert"
          class="alert alert-soft {alert.severity === 'critical' ? 'alert-error' : 'alert-warning'}"
        >
          <span>{alert.message}</span>
          <span class="text-sm opacity-70">since {since(alert.since)}</span>
        </div>
      {:else}
        <p class="flex items-center gap-2 text-base-content/70">
          <span class="status status-success"></span> Nothing to report.
        </p>
      {/each}
    </div>
  </section>

  {#if config}
    <section class="card border border-base-300 bg-base-100" aria-labelledby="rules-title">
      <div class="card-body gap-4">
        <h2 id="rules-title" class="card-title">When to alert</h2>
        <div class="grid grid-cols-[repeat(auto-fill,minmax(16rem,1fr))] gap-x-6 gap-y-4">
          <OptionalNumber bind:value={config.alerts.cpu_high} label="CPU above" unit="°C" fallback={85} />
          <OptionalNumber
            bind:value={config.alerts.disk_high}
            label="HDD or SSD above"
            unit="°C"
            fallback={45}
          />
          <OptionalNumber bind:value={config.alerts.nvme_high} label="NVMe above" unit="°C" fallback={70} />
          <OptionalNumber bind:value={config.alerts.disk_low} label="Disk below" unit="°C" fallback={10} />
          <OptionalNumber
            bind:value={config.alerts.fan_min_rpm}
            label="Fan below"
            unit="RPM"
            fallback={150}
          />
          <OptionalNumber
            bind:value={config.alerts.fan_slowdown_percent}
            label="Fan slower than at its calibration by"
            unit="%"
            fallback={15}
          />
        </div>
        <div class="grid grid-cols-[repeat(auto-fill,minmax(16rem,1fr))] gap-x-6">
          <fieldset class="fieldset">
            <legend class="fieldset-legend">BMC unreachable after (failed commands)</legend>
            <input class="input w-full" type="number" min="0" bind:value={config.alerts.ipmi_failures} />
          </fieldset>
          <fieldset class="fieldset">
            <legend class="fieldset-legend">Repeat an alert every (minutes, 0 = never)</legend>
            <input class="input w-full" type="number" min="0" bind:value={config.alerts.repeat_minutes} />
          </fieldset>
        </div>
        <div class="grid gap-3">
          <label class="label cursor-pointer gap-3 whitespace-normal text-base-content"
            ><input
              type="checkbox"
              class="toggle toggle-sm toggle-primary"
              bind:checked={config.alerts.smart_health}
            />
            Disk failing, or its SMART error counters going up</label
          >
          <label class="label cursor-pointer gap-3 whitespace-normal text-base-content"
            ><input
              type="checkbox"
              class="toggle toggle-sm toggle-primary"
              bind:checked={config.alerts.bmc_events}
            />
            New warning or failure in the BMC log</label
          >
          <label class="label cursor-pointer gap-3 whitespace-normal text-base-content"
            ><input
              type="checkbox"
              class="toggle toggle-sm toggle-primary"
              bind:checked={config.alerts.notify_resolved}
            /> Notify when an alert clears</label
          >
        </div>
        <p class="text-sm text-base-content/60">
          A zone running at failsafe speed and a disk that cannot be read always raise an alert.
        </p>
      </div>
    </section>

    <section class="card border border-base-300 bg-base-100" aria-labelledby="channels-title">
      <div class="card-body gap-4">
        <h2 id="channels-title" class="card-title">Where to send them</h2>
        {#each config.notifications as channel, i (i)}
          <div class="grid gap-2 rounded-box border border-base-300 p-4">
            <div class="flex flex-wrap items-center justify-between gap-3">
              <label class="label cursor-pointer gap-3 text-base-content"
                ><input
                  type="checkbox"
                  class="toggle toggle-sm toggle-success"
                  bind:checked={channel.enabled}
                />
                <strong>{KINDS.find((k) => k.type === channel.type)?.label}</strong></label
              >
              <div class="flex flex-wrap gap-2">
                <button class="btn btn-sm" onclick={() => test(channel, i)}>Send a test</button>
                <button
                  class="btn btn-sm btn-error btn-outline"
                  onclick={() => config.notifications.splice(i, 1)}>Remove</button
                >
              </div>
            </div>
            <div class="grid grid-cols-[repeat(auto-fill,minmax(14rem,1fr))] gap-x-4">
              {#if channel.type === 'discord' || channel.type === 'webhook'}
                <fieldset class="fieldset col-span-full">
                  <legend class="fieldset-legend">URL</legend>
                  <input
                    class="input w-full"
                    bind:value={channel.url}
                    placeholder="https://…"
                    autocomplete="off"
                  />
                </fieldset>
              {:else if channel.type === 'ntfy'}
                <fieldset class="fieldset">
                  <legend class="fieldset-legend">Server</legend>
                  <input class="input w-full" bind:value={channel.server} />
                </fieldset>
                <fieldset class="fieldset">
                  <legend class="fieldset-legend">Topic</legend>
                  <input class="input w-full" bind:value={channel.topic} />
                </fieldset>
                <fieldset class="fieldset">
                  <legend class="fieldset-legend">Access token (optional)</legend>
                  <input class="input w-full" bind:value={channel.token} autocomplete="off" />
                </fieldset>
              {:else if channel.type === 'gotify'}
                <fieldset class="fieldset">
                  <legend class="fieldset-legend">Server</legend>
                  <input class="input w-full" bind:value={channel.server} placeholder="https://…" />
                </fieldset>
                <fieldset class="fieldset">
                  <legend class="fieldset-legend">Application token</legend>
                  <input class="input w-full" bind:value={channel.token} autocomplete="off" />
                </fieldset>
              {/if}
            </div>
            {#if testResult[i]}<p class="text-sm text-base-content/70">{testResult[i]}</p>{/if}
          </div>
        {:else}
          <p class="text-base-content/60">No destination yet: alerts only appear here and in the journal.</p>
        {/each}
        <div class="join">
          <label class="sr-only" for="new-kind">Destination type</label>
          <select id="new-kind" class="select join-item w-48" bind:value={newKind}>
            {#each KINDS as k (k.type)}<option value={k.type}>{k.label}</option>{/each}
          </select>
          <button class="btn join-item" onclick={() => config.notifications.push(blank(newKind))}
            >Add destination</button
          >
        </div>
      </div>
    </section>
  {/if}
</div>
