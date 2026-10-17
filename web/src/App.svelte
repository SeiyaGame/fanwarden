<script lang="ts">
  import { onMount } from 'svelte'
  import Rotor from './components/Rotor.svelte'
  import { api, setUnauthorizedHandler } from './lib/api'
  import { configStore } from './lib/configStore.svelte'
  import { live } from './lib/live.svelte'
  import Alerts from './pages/Alerts.svelte'
  import Curves from './pages/Curves.svelte'
  import History from './pages/History.svelte'
  import Journal from './pages/Journal.svelte'
  import Login from './pages/Login.svelte'
  import Overview from './pages/Overview.svelte'
  import Settings from './pages/Settings.svelte'

  const PAGES = [
    { path: '', label: 'Overview', component: Overview },
    { path: 'curves', label: 'Curves', component: Curves },
    { path: 'history', label: 'History', component: History },
    { path: 'alerts', label: 'Alerts', component: Alerts },
    { path: 'settings', label: 'Settings', component: Settings },
    { path: 'journal', label: 'Journal', component: Journal },
  ]

  let auth = $state<'loading' | 'setup' | 'login' | 'ok'>('loading')
  let route = $state(location.hash.replace(/^#\/?/, ''))
  const page = $derived(PAGES.find((p) => p.path === route) ?? PAGES[0])
  const snapshot = $derived(live.snapshot)
  const cpuRpm = $derived(Math.max(0, ...(snapshot?.fans.map((f) => f.rpm ?? 0) ?? [0])) || null)

  async function checkAuth() {
    try {
      const status = await api.authStatus()
      auth = status.authenticated ? 'ok' : status.setup_required ? 'setup' : 'login'
    } catch {
      auth = 'login'
    }
  }

  $effect(() => {
    if (auth !== 'ok') return
    live.start()
    configStore.load()
    return () => live.stop()
  })

  onMount(() => {
    setUnauthorizedHandler(() => (auth = 'login'))
    checkAuth()
    const onHash = () => (route = location.hash.replace(/^#\/?/, ''))
    window.addEventListener('hashchange', onHash)
    const beforeUnload = (e: BeforeUnloadEvent) => {
      if (configStore.dirty) e.preventDefault()
    }
    window.addEventListener('beforeunload', beforeUnload)
    return () => {
      window.removeEventListener('hashchange', onHash)
      window.removeEventListener('beforeunload', beforeUnload)
    }
  })

  async function logout() {
    await api.logout()
    auth = 'login'
  }
</script>

{#if auth === 'loading'}
  <div></div>
{:else if auth !== 'ok'}
  <Login setup={auth === 'setup'} onDone={() => (auth = 'ok')} />
{:else}
  <div class="shell">
    <nav aria-label="Sections">
      <a class="brand" href="#/">
        <Rotor rpm={cpuRpm} size={30} color="var(--warm)" />
        <span>Fanwarden</span>
      </a>
      <ul>
        {#each PAGES as p (p.path)}
          <li>
            <a href="#/{p.path}" aria-current={p === page ? 'page' : undefined}>
              {p.label}
              {#if p.path === 'alerts' && snapshot?.alerts.length}
                <span class="count figure" aria-label="{snapshot.alerts.length} active"
                  >{snapshot.alerts.length}</span
                >
              {/if}
            </a>
          </li>
        {/each}
      </ul>
      <div class="status">
        {#if snapshot?.simulate}<p class="flag">Simulated hardware</p>{/if}
        {#if snapshot?.dry_run}<p class="flag">Dry run: speeds are not applied</p>{/if}
        <p class="soft">
          <span class="link" class:up={live.connected}></span>
          {live.connected ? 'Live' : 'Reconnecting…'}
        </p>
        <button class="quiet" onclick={logout}>Sign out</button>
      </div>
    </nav>

    <main>
      <page.component />
    </main>

    {#if configStore.dirty || configStore.error}
      <div class="savebar" role="region" aria-label="Unsaved changes">
        {#if configStore.error}
          <p class="error">Not saved: {configStore.error}</p>
        {:else}
          <p>You have unsaved changes.</p>
        {/if}
        <div class="actions">
          <button class="quiet" onclick={() => configStore.discard()}>Discard</button>
          <button onclick={() => configStore.save()} disabled={configStore.saving}>
            {configStore.saving ? 'Saving…' : 'Save changes'}
          </button>
        </div>
      </div>
    {/if}
  </div>
{/if}

<style>
  .shell {
    display: grid;
    grid-template-columns: 210px minmax(0, 1fr);
    min-height: 100dvh;
  }

  nav {
    position: sticky;
    top: 0;
    height: 100dvh;
    display: flex;
    flex-direction: column;
    gap: 1.5rem;
    padding: 1.25rem 1rem;
    border-right: 1px solid var(--rule);
    background: var(--panel-sunk);
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 0.6rem;
    text-decoration: none;
    font-weight: 600;
    font-size: var(--step-1);
  }

  nav ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 2px;
  }

  nav ul a {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 0.45rem 0.7rem;
    border-radius: var(--radius-s);
    text-decoration: none;
    color: var(--ink-soft);
    font-weight: 500;
  }

  nav ul a:hover {
    color: var(--ink);
  }

  nav ul a[aria-current='page'] {
    background: var(--panel);
    color: var(--ink);
    box-shadow: inset 3px 0 0 var(--warm);
  }

  .count {
    background: var(--critical);
    color: #fff;
    border-radius: 999px;
    min-width: 1.4rem;
    text-align: center;
    font-size: var(--step--1);
    padding: 0 0.35rem;
  }

  .status {
    margin-top: auto;
    display: grid;
    gap: 0.6rem;
    font-size: var(--step--1);
  }

  .flag {
    color: var(--ink);
    font-weight: 600;
    border-left: 3px solid var(--warm);
    padding-left: 0.5rem;
  }

  .link {
    display: inline-block;
    width: 0.5rem;
    height: 0.5rem;
    border-radius: 50%;
    background: var(--hot);
    margin-right: 0.3rem;
  }

  .link.up {
    background: var(--mild);
  }

  main {
    padding: var(--gutter);
    padding-bottom: calc(var(--gutter) + 5rem);
    max-width: 1280px;
    width: 100%;
  }

  .savebar {
    position: fixed;
    left: 50%;
    bottom: calc(1rem + env(safe-area-inset-bottom, 0px));
    transform: translateX(-50%);
    width: min(calc(100% - 2rem), 640px);
    display: flex;
    justify-content: space-between;
    align-items: center;
    flex-wrap: wrap;
    gap: 0.75rem;
    padding: 0.75rem 1rem;
    background: var(--control);
    color: var(--control-ink);
    border-radius: var(--radius-m);
    box-shadow: 0 8px 30px rgb(0 0 0 / 0.25);
  }

  .savebar .actions {
    display: flex;
    gap: 0.5rem;
  }

  .savebar button {
    background: var(--control-ink);
    color: var(--control);
    border-color: var(--control-ink);
  }

  .savebar button.quiet {
    background: transparent;
    color: var(--control-ink);
    border-color: color-mix(in srgb, var(--control-ink) 40%, transparent);
  }

  .savebar .error {
    color: #ffb4a8;
  }

  @media (max-width: 760px) {
    .shell {
      grid-template-columns: 1fr;
    }

    nav {
      position: static;
      height: auto;
      flex-direction: row;
      flex-wrap: wrap;
      align-items: center;
      gap: 0.75rem 1rem;
      border-right: 0;
      border-bottom: 1px solid var(--rule);
      padding: 0.75rem var(--gutter);
    }

    nav ul {
      display: flex;
      flex-wrap: wrap;
      order: 3;
      width: 100%;
    }

    .status {
      margin: 0 0 0 auto;
      display: flex;
      align-items: center;
      gap: 0.75rem;
    }
  }
</style>
