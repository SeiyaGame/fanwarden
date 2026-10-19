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

<div class="page">
  <header class="head">
    <h1>Settings</h1>
    <div class="reload">
      <button class="quiet" onclick={reload} disabled={reloading}>
        {configStore.dirty ? 'Discard my changes and reload the file' : 'Reload the configuration file'}
      </button>
      <p class="soft">Applies changes made by hand in config.toml, without restarting.</p>
    </div>
  </header>
  {#if reloadMessage}
    <p class:error={!reloadMessage.ok} role="status">{reloadMessage.text}</p>
  {/if}

  {#if update}
    <section class="panel" aria-labelledby="update-title">
      <h2 id="update-title">
        {update.available && update.latest ? `Version ${update.latest.version} is available` : 'Version'}
      </h2>
      <p>
        This is fanwarden {update.current}{#if update.build}&nbsp;({update.build}){/if}.
        {#if !update.enabled}
          Checking for new versions is off.
        {:else if update.error}
          <span class="error">Cannot check for a new version: {update.error}</span>
        {:else if update.checked && !update.available}
          It is the latest release (checked {new Date(update.checked * 1000).toLocaleString()}).
        {/if}
      </p>
      {#if update.available && update.latest}
        {#each notes as section, i (i)}
          <div class="notes">
            {#if section.title}<h3>{section.title}</h3>{/if}
            <ul>
              {#each section.items as item, j (j)}<li>{item}</li>{/each}
            </ul>
          </div>
        {/each}
        <p><a href={update.latest.url} target="_blank" rel="noreferrer">Release page on GitHub</a></p>
        <div class="how">
          <p>To update, run the installer again with the folder fanwarden is installed in:</p>
          <pre><code
              >curl -fsSLO https://raw.githubusercontent.com/SeiyaGame/fanwarden/main/install.sh
sudo bash install.sh --dir /opt/fanwarden</code
            ></pre>
          <p>With Docker: <code>docker compose pull && docker compose up -d</code>.</p>
        </div>
      {/if}
    </section>
  {/if}

  {#if config}
    <section class="panel" aria-labelledby="control-title">
      <h2 id="control-title">Control</h2>
      <div class="grid-fields">
        <label class="field"
          >Check temperatures every (seconds)
          <input type="number" min="2" bind:value={config.general.interval_secs} /></label
        >
        <label class="field"
          >Failsafe speed (%) <input
            type="number"
            min="0"
            max="100"
            bind:value={config.general.failsafe_duty}
          /></label
        >
        <label class="field"
          >Resend speeds every (seconds) <input
            type="number"
            min="10"
            bind:value={config.general.reapply_secs}
          /></label
        >
        <label class="field"
          >When the service stops, hand fans back in mode
          <select bind:value={config.general.exit_fan_mode}>
            <option value="standard">Standard</option>
            <option value="optimal">Optimal</option>
            <option value="heavy_io">Heavy IO</option>
            <option value="full">Full</option>
          </select>
        </label>
        <label class="check"
          ><input type="checkbox" bind:checked={config.general.dry_run} /> Dry run: compute speeds without applying
          them</label
        >
      </div>
    </section>

    <section class="panel" aria-labelledby="regulation-title">
      <h2 id="regulation-title">Target temperature mode</h2>
      <p class="soft">
        A source set to hold a temperature follows its curve again when it runs this far above its target,
        without waiting for the regulation.
      </p>
      <div class="grid-fields">
        <label class="field"
          >Safety net for disks and other sensors (°C above target)
          <input
            type="number"
            min="1"
            max="30"
            step="0.5"
            bind:value={config.regulation.safety_margin_disks}
          /></label
        >
        <label class="field"
          >Safety net for the CPU (°C above target)
          <input
            type="number"
            min="1"
            max="30"
            step="0.5"
            bind:value={config.regulation.safety_margin_cpu}
          /></label
        >
      </div>
    </section>

    <section class="panel" aria-labelledby="ipmi-title">
      <h2 id="ipmi-title">BMC connection</h2>
      <div class="grid-fields">
        <label class="field"
          >Board
          <select bind:value={config.ipmi.target}>
            <option value="local">This machine (/dev/ipmi0)</option>
            <option value="remote">Over the network</option>
          </select>
        </label>
        {#if config.ipmi.target === 'remote'}
          <label class="field"
            >BMC address <input bind:value={config.ipmi.host} placeholder="192.168.1.20" /></label
          >
          <label class="field">User <input bind:value={config.ipmi.username} autocomplete="off" /></label>
          <label class="field"
            >Password <input
              type="password"
              bind:value={config.ipmi.password}
              autocomplete="new-password"
            /></label
          >
          <label class="field"
            >Protocol
            <select bind:value={config.ipmi.interface}>
              <option value="lanplus">IPMI 2.0 (lanplus)</option>
              <option value="lan">IPMI 1.5 (lan)</option>
            </select>
          </label>
        {/if}
        <label class="field">ipmitool command <input bind:value={config.ipmi.ipmitool_path} /></label>
        <label class="field"
          >Command timeout (seconds) <input
            type="number"
            min="1"
            bind:value={config.ipmi.timeout_secs}
          /></label
        >
      </div>
    </section>

    <section class="panel" aria-labelledby="disks-title">
      <h2 id="disks-title">Disks</h2>
      <div class="grid-fields">
        <label class="check"
          ><input type="checkbox" bind:checked={config.disks.enabled} /> Read disk temperatures</label
        >
        <label class="check"
          ><input type="checkbox" bind:checked={config.disks.skip_standby} /> Let sleeping disks sleep</label
        >
        <label class="field"
          >Read disks every (seconds) <input
            type="number"
            min="5"
            bind:value={config.disks.interval_secs}
          /></label
        >
        <label class="field"
          >Ignore disks matching (regex) <input
            bind:value={config.disks.exclude}
            placeholder="^(zram|loop)"
          /></label
        >
        <label class="field">smartctl command <input bind:value={config.disks.smartctl_path} /></label>
        <label class="field">lsblk command <input bind:value={config.disks.lsblk_path} /></label>
      </div>
    </section>

    <section class="panel" aria-labelledby="data-title">
      <h2 id="data-title">History and metrics</h2>
      <div class="grid-fields">
        <label class="check"
          ><input type="checkbox" bind:checked={config.history.enabled} /> Keep a history</label
        >
        <label class="field"
          >Keep history for (days) <input
            type="number"
            min="1"
            bind:value={config.history.retention_days}
          /></label
        >
        <label class="check"
          ><input type="checkbox" bind:checked={config.web.metrics_enabled} /> Publish Prometheus metrics on /metrics</label
        >
        <label class="check"
          ><input type="checkbox" bind:checked={config.general.check_updates} /> Check GitHub for a new version
          twice a day</label
        >
        <label class="field"
          >Web address (applies after a restart) <input bind:value={config.web.listen} /></label
        >
        <label class="field"
          >Stay signed in for (hours) <input
            type="number"
            min="1"
            bind:value={config.web.session_hours}
          /></label
        >
      </div>
    </section>
  {/if}

  <section class="panel" aria-labelledby="password-title">
    <h2 id="password-title">Password</h2>
    <form class="grid-fields" onsubmit={changePassword}>
      <label class="field"
        >Current password <input
          type="password"
          bind:value={current}
          autocomplete="current-password"
          required
        /></label
      >
      <label class="field"
        >New password <input
          type="password"
          bind:value={next}
          minlength="8"
          autocomplete="new-password"
          required
        /></label
      >
      <label class="field"
        >New password again
        <input
          type="password"
          bind:value={confirm}
          minlength="8"
          autocomplete="new-password"
          required
        /></label
      >
      <div class="submit"><button type="submit">Change password</button></div>
    </form>
    {#if passwordMessage}
      <p class:error={!passwordMessage.ok}>{passwordMessage.text}</p>
    {/if}
  </section>
</div>

<style>
  .page {
    display: grid;
    gap: var(--gutter);
  }

  .head {
    display: flex;
    justify-content: space-between;
    align-items: start;
    flex-wrap: wrap;
    gap: 1rem;
  }

  .reload {
    display: grid;
    justify-items: end;
    gap: 0.3rem;
    font-size: var(--step--1);
  }

  section.panel {
    display: grid;
    gap: 1rem;
  }

  .submit {
    align-self: end;
  }

  section.panel p,
  .notes h3,
  .notes ul {
    margin: 0;
  }

  .notes h3 {
    font-size: var(--step-0);
  }

  .notes ul {
    padding-left: 1.2rem;
  }

  .how {
    display: grid;
    gap: 0.5rem;
  }

  pre {
    margin: 0;
    padding: 0.75rem;
    overflow-x: auto;
    background: var(--panel-sunk);
    border-radius: var(--radius-s);
  }

  .error {
    color: var(--critical);
  }
</style>
