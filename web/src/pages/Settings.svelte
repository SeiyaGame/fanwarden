<script lang="ts">
  import { api } from '../lib/api'
  import { configStore } from '../lib/configStore.svelte'
  import { parseNotes } from '../lib/releaseNotes'
  import { updates } from '../lib/update.svelte'

  const config = $derived(configStore.draft)
  const update = $derived(updates.status)
  const notes = $derived(update?.latest ? parseNotes(update.latest.notes) : [])

  let current = $state('')
  let next = $state('')
  let confirm = $state('')
  let passwordMessage = $state<{ ok: boolean; text: string } | null>(null)
  let reloadMessage = $state<{ ok: boolean; text: string } | null>(null)
  let reloading = $state(false)

  async function reload() {
    reloading = true
    reloadMessage = null
    try {
      await configStore.reload()
      reloadMessage = { ok: true, text: `Configuration file reloaded at ${new Date().toLocaleTimeString()}.` }
    } catch (e) {
      reloadMessage = {
        ok: false,
        text: `Not reloaded, the running configuration is unchanged. ${e instanceof Error ? e.message : e}`,
      }
    } finally {
      reloading = false
    }
  }

  async function changePassword(event: SubmitEvent) {
    event.preventDefault()
    if (next !== confirm) {
      passwordMessage = { ok: false, text: 'The two new passwords are different.' }
      return
    }
    try {
      await api.changePassword(current, next)
      passwordMessage = { ok: true, text: 'Password changed. Other sessions are logged out.' }
      current = next = confirm = ''
    } catch (e) {
      passwordMessage = { ok: false, text: e instanceof Error ? e.message : String(e) }
    }
  }
</script>

<div class="grid gap-6">
  <header class="flex flex-wrap items-start justify-between gap-4">
    <h1 class="text-2xl font-semibold">Settings</h1>
    <div class="grid justify-items-start gap-1 text-sm sm:justify-items-end">
      <button class="btn btn-sm" onclick={reload} disabled={reloading}>
        {#if reloading}<span class="loading loading-spinner loading-xs"></span>{/if}
        {configStore.dirty ? 'Discard my changes and reload the file' : 'Reload the configuration file'}
      </button>
      <p class="text-base-content/60">Applies changes made by hand in config.toml, without restarting.</p>
    </div>
  </header>
  {#if reloadMessage}
    <div role="status" class="alert alert-soft {reloadMessage.ok ? 'alert-success' : 'alert-error'}">
      {reloadMessage.text}
    </div>
  {/if}

  {#if update}
    <section
      class="card border bg-base-100 {update.available ? 'border-success' : 'border-base-300'}"
      aria-labelledby="update-title"
    >
      <div class="card-body gap-3">
        <h2 id="update-title" class="card-title">
          {update.available && update.latest ? `Version ${update.latest.version} is available` : 'Version'}
          {#if update.available}<span class="badge badge-success badge-sm">new</span>{/if}
        </h2>
        <p>
          This is fanwarden {update.current}{#if update.build}&nbsp;({update.build}){/if}.
          {#if !update.enabled}
            Checking for new versions is off.
          {:else if update.error}
            <span class="text-error">Cannot check for a new version: {update.error}</span>
          {:else if update.checked && !update.available}
            It is the latest release (checked {new Date(update.checked * 1000).toLocaleString()}).
          {/if}
        </p>
        {#if update.available && update.latest}
          {#each notes as section, i (i)}
            <div>
              {#if section.title}<h3 class="font-semibold">{section.title}</h3>{/if}
              <ul class="list-disc pl-5">
                {#each section.items as item, j (j)}<li>{item}</li>{/each}
              </ul>
            </div>
          {/each}
          <p>
            <a class="link link-primary" href={update.latest.url} target="_blank" rel="noreferrer"
              >Release page on GitHub</a
            >
          </p>
          <p>To update, run the installer again with the folder fanwarden is installed in:</p>
          <div class="mockup-code text-sm">
            <pre data-prefix="$"><code
                >curl -fsSLO https://raw.githubusercontent.com/SeiyaGame/fanwarden/main/install.sh</code
              ></pre>
            <pre data-prefix="$"><code>sudo bash install.sh --dir /opt/fanwarden</code></pre>
          </div>
          <p>With Docker: <code class="font-mono">docker compose pull && docker compose up -d</code>.</p>
        {/if}
      </div>
    </section>
  {/if}

  {#if config}
    <section class="card border border-base-300 bg-base-100" aria-labelledby="control-title">
      <div class="card-body">
        <h2 id="control-title" class="card-title">Control</h2>
        <div class="grid grid-cols-[repeat(auto-fill,minmax(15rem,1fr))] gap-x-4">
          <fieldset class="fieldset">
            <legend class="fieldset-legend">Check temperatures every (seconds)</legend>
            <input class="input w-full" type="number" min="2" bind:value={config.general.interval_secs} />
          </fieldset>
          <fieldset class="fieldset">
            <legend class="fieldset-legend">Failsafe speed (%)</legend>
            <input
              class="input w-full"
              type="number"
              min="0"
              max="100"
              bind:value={config.general.failsafe_duty}
            />
          </fieldset>
          <fieldset class="fieldset">
            <legend class="fieldset-legend">Resend speeds every (seconds)</legend>
            <input class="input w-full" type="number" min="10" bind:value={config.general.reapply_secs} />
          </fieldset>
          <fieldset class="fieldset">
            <legend class="fieldset-legend">When the service stops, hand fans back in mode</legend>
            <select class="select w-full" bind:value={config.general.exit_fan_mode}>
              <option value="standard">Standard</option>
              <option value="optimal">Optimal</option>
              <option value="heavy_io">Heavy IO</option>
              <option value="full">Full</option>
            </select>
          </fieldset>
        </div>
        <label class="label mt-2 cursor-pointer gap-3 whitespace-normal text-base-content"
          ><input
            type="checkbox"
            class="toggle toggle-sm toggle-warning"
            bind:checked={config.general.dry_run}
          />
          Dry run: compute speeds without applying them</label
        >
      </div>
    </section>

    <section class="card border border-base-300 bg-base-100" aria-labelledby="regulation-title">
      <div class="card-body">
        <h2 id="regulation-title" class="card-title">Target temperature mode</h2>
        <p class="text-sm text-base-content/70">
          A source set to hold a temperature follows its curve again when it runs this far above its target,
          without waiting for the regulation.
        </p>
        <div class="grid grid-cols-[repeat(auto-fill,minmax(15rem,1fr))] gap-x-4">
          <fieldset class="fieldset">
            <legend class="fieldset-legend">Safety net for disks and other sensors (°C above target)</legend>
            <input
              class="input w-full"
              type="number"
              min="1"
              max="30"
              step="0.5"
              bind:value={config.regulation.safety_margin_disks}
            />
          </fieldset>
          <fieldset class="fieldset">
            <legend class="fieldset-legend">Safety net for the CPU (°C above target)</legend>
            <input
              class="input w-full"
              type="number"
              min="1"
              max="30"
              step="0.5"
              bind:value={config.regulation.safety_margin_cpu}
            />
          </fieldset>
        </div>
      </div>
    </section>

    <section class="card border border-base-300 bg-base-100" aria-labelledby="ipmi-title">
      <div class="card-body">
        <h2 id="ipmi-title" class="card-title">BMC connection</h2>
        <div class="grid grid-cols-[repeat(auto-fill,minmax(15rem,1fr))] gap-x-4">
          <fieldset class="fieldset">
            <legend class="fieldset-legend">Board</legend>
            <select class="select w-full" bind:value={config.ipmi.target}>
              <option value="local">This machine (/dev/ipmi0)</option>
              <option value="remote">Over the network</option>
            </select>
          </fieldset>
          {#if config.ipmi.target === 'remote'}
            <fieldset class="fieldset">
              <legend class="fieldset-legend">BMC address</legend>
              <input class="input w-full" bind:value={config.ipmi.host} placeholder="192.168.1.20" />
            </fieldset>
            <fieldset class="fieldset">
              <legend class="fieldset-legend">User</legend>
              <input class="input w-full" bind:value={config.ipmi.username} autocomplete="off" />
            </fieldset>
            <fieldset class="fieldset">
              <legend class="fieldset-legend">Password</legend>
              <input
                class="input w-full"
                type="password"
                bind:value={config.ipmi.password}
                autocomplete="new-password"
              />
            </fieldset>
            <fieldset class="fieldset">
              <legend class="fieldset-legend">Protocol</legend>
              <select class="select w-full" bind:value={config.ipmi.interface}>
                <option value="lanplus">IPMI 2.0 (lanplus)</option>
                <option value="lan">IPMI 1.5 (lan)</option>
              </select>
            </fieldset>
          {/if}
          <fieldset class="fieldset">
            <legend class="fieldset-legend">ipmitool command</legend>
            <input class="input w-full" bind:value={config.ipmi.ipmitool_path} />
          </fieldset>
          <fieldset class="fieldset">
            <legend class="fieldset-legend">Command timeout (seconds)</legend>
            <input class="input w-full" type="number" min="1" bind:value={config.ipmi.timeout_secs} />
          </fieldset>
        </div>
      </div>
    </section>

    <section class="card border border-base-300 bg-base-100" aria-labelledby="disks-title">
      <div class="card-body">
        <h2 id="disks-title" class="card-title">Disks</h2>
        <div class="flex flex-wrap gap-x-8 gap-y-3">
          <label class="label cursor-pointer gap-3 text-base-content"
            ><input
              type="checkbox"
              class="toggle toggle-sm toggle-primary"
              bind:checked={config.disks.enabled}
            />
            Read disk temperatures</label
          >
          <label class="label cursor-pointer gap-3 text-base-content"
            ><input
              type="checkbox"
              class="toggle toggle-sm toggle-primary"
              bind:checked={config.disks.skip_standby}
            /> Let sleeping disks sleep</label
          >
        </div>
        <div class="grid grid-cols-[repeat(auto-fill,minmax(15rem,1fr))] gap-x-4">
          <fieldset class="fieldset">
            <legend class="fieldset-legend">Read disks every (seconds)</legend>
            <input class="input w-full" type="number" min="5" bind:value={config.disks.interval_secs} />
          </fieldset>
          <fieldset class="fieldset">
            <legend class="fieldset-legend">Ignore disks matching (regex)</legend>
            <input
              class="input w-full font-mono"
              bind:value={config.disks.exclude}
              placeholder="^(zram|loop)"
            />
          </fieldset>
          <fieldset class="fieldset">
            <legend class="fieldset-legend">smartctl command</legend>
            <input class="input w-full" bind:value={config.disks.smartctl_path} />
          </fieldset>
          <fieldset class="fieldset">
            <legend class="fieldset-legend">lsblk command</legend>
            <input class="input w-full" bind:value={config.disks.lsblk_path} />
          </fieldset>
        </div>
      </div>
    </section>

    <section class="card border border-base-300 bg-base-100" aria-labelledby="data-title">
      <div class="card-body">
        <h2 id="data-title" class="card-title">History and metrics</h2>
        <div class="grid gap-3">
          <label class="label cursor-pointer gap-3 whitespace-normal text-base-content"
            ><input
              type="checkbox"
              class="toggle toggle-sm toggle-primary"
              bind:checked={config.history.enabled}
            />
            Keep a history</label
          >
          <label class="label cursor-pointer gap-3 whitespace-normal text-base-content"
            ><input
              type="checkbox"
              class="toggle toggle-sm toggle-primary"
              bind:checked={config.web.metrics_enabled}
            /> Publish Prometheus metrics on /metrics</label
          >
          <label class="label cursor-pointer gap-3 whitespace-normal text-base-content"
            ><input
              type="checkbox"
              class="toggle toggle-sm toggle-primary"
              bind:checked={config.general.check_updates}
            /> Check GitHub for a new version twice a day</label
          >
        </div>
        <div class="grid grid-cols-[repeat(auto-fill,minmax(15rem,1fr))] gap-x-4">
          <fieldset class="fieldset">
            <legend class="fieldset-legend">Keep history for (days)</legend>
            <input class="input w-full" type="number" min="1" bind:value={config.history.retention_days} />
          </fieldset>
          <fieldset class="fieldset">
            <legend class="fieldset-legend">Web address (applies after a restart)</legend>
            <input class="input w-full" bind:value={config.web.listen} />
          </fieldset>
          <fieldset class="fieldset">
            <legend class="fieldset-legend">Stay signed in for (hours)</legend>
            <input class="input w-full" type="number" min="1" bind:value={config.web.session_hours} />
          </fieldset>
        </div>
      </div>
    </section>
  {/if}

  <section class="card border border-base-300 bg-base-100" aria-labelledby="password-title">
    <div class="card-body">
      <h2 id="password-title" class="card-title">Password</h2>
      <form
        class="grid grid-cols-[repeat(auto-fill,minmax(15rem,1fr))] items-end gap-x-4"
        onsubmit={changePassword}
      >
        <fieldset class="fieldset">
          <legend class="fieldset-legend">Current password</legend>
          <input
            class="input w-full"
            type="password"
            bind:value={current}
            autocomplete="current-password"
            required
          />
        </fieldset>
        <fieldset class="fieldset">
          <legend class="fieldset-legend">New password</legend>
          <input
            class="input w-full"
            type="password"
            bind:value={next}
            minlength="8"
            autocomplete="new-password"
            required
          />
        </fieldset>
        <fieldset class="fieldset">
          <legend class="fieldset-legend">New password again</legend>
          <input
            class="input w-full"
            type="password"
            bind:value={confirm}
            minlength="8"
            autocomplete="new-password"
            required
          />
        </fieldset>
        <div class="py-1"><button type="submit" class="btn btn-primary">Change password</button></div>
      </form>
      {#if passwordMessage}
        <div role="status" class="alert alert-soft {passwordMessage.ok ? 'alert-success' : 'alert-error'}">
          {passwordMessage.text}
        </div>
      {/if}
    </div>
  </section>
</div>
