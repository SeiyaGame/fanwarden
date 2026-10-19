<script lang="ts">
  import { onMount } from 'svelte'
  import Rotor from './components/Rotor.svelte'
  import { api, setUnauthorizedHandler } from './lib/api'
  import { configStore } from './lib/configStore.svelte'
  import { live } from './lib/live.svelte'
  import { updates } from './lib/update.svelte'
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
  let drawerOpen = $state(false)
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
    updates.start()
    return () => {
      live.stop()
      updates.stop()
    }
  })

  onMount(() => {
    setUnauthorizedHandler(() => (auth = 'login'))
    checkAuth()
    const onHash = () => {
      route = location.hash.replace(/^#\/?/, '')
      drawerOpen = false
    }
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
  <div class="drawer lg:drawer-open min-h-dvh bg-base-200">
    <input id="nav-drawer" type="checkbox" class="drawer-toggle" bind:checked={drawerOpen} />

    <div class="drawer-content flex min-w-0 flex-col">
      <header class="navbar sticky top-0 z-20 border-b border-base-300 bg-base-100 lg:hidden">
        <label for="nav-drawer" class="btn btn-square btn-ghost" aria-label="Open the menu">
          <svg viewBox="0 0 24 24" class="size-6" fill="none" stroke="currentColor" stroke-width="2"
            ><path d="M4 6h16M4 12h16M4 18h16" /></svg
          >
        </label>
        <a class="flex items-center gap-2 px-2 text-lg font-semibold" href="#/">
          <Rotor rpm={cpuRpm} size={26} color="var(--color-accent)" />
          Fanwarden
        </a>
        {#if snapshot?.alerts.length}
          <a href="#/alerts" class="badge badge-error ml-auto font-figures">{snapshot.alerts.length}</a>
        {/if}
      </header>

      <main class="mx-auto w-full max-w-7xl flex-1 p-4 pb-28 sm:p-6 lg:p-8 lg:pb-28">
        <svelte:boundary>
          <page.component />
          {#snippet failed(error, reset)}
            <div role="alert" class="alert alert-error flex-col items-start">
              <h1 class="text-lg font-semibold">This page stopped working</h1>
              <p>{error instanceof Error ? error.message : String(error)}</p>
              <p class="opacity-80">
                The service keeps controlling the fans. Report this message if it comes back.
              </p>
              <button class="btn btn-sm" onclick={reset}>Try again</button>
            </div>
          {/snippet}
        </svelte:boundary>
      </main>

      {#if configStore.dirty || configStore.error}
        <div role="region" aria-label="Unsaved changes" class="fixed inset-x-0 bottom-0 z-30 p-3 lg:left-60">
          <div
            class="alert mx-auto max-w-3xl shadow-lg {configStore.error
              ? 'alert-error'
              : 'border-base-300 bg-base-100'}"
          >
            <span>{configStore.error ? `Not saved: ${configStore.error}` : 'You have unsaved changes.'}</span>
            <div class="flex gap-2">
              <button class="btn btn-ghost btn-sm" onclick={() => configStore.discard()}>Discard</button>
              <button
                class="btn btn-primary btn-sm"
                onclick={() => configStore.save()}
                disabled={configStore.saving}
              >
                {#if configStore.saving}<span class="loading loading-spinner loading-xs"></span>{/if}
                Save changes
              </button>
            </div>
          </div>
        </div>
      {/if}
    </div>

    <div class="drawer-side z-40">
      <label for="nav-drawer" aria-label="Close the menu" class="drawer-overlay"></label>
      <nav
        aria-label="Sections"
        class="flex min-h-full w-60 flex-col gap-4 border-r border-base-300 bg-base-100 p-4"
      >
        <a class="flex items-center gap-2.5 px-2 text-lg font-semibold" href="#/">
          <Rotor rpm={cpuRpm} size={30} color="var(--color-accent)" />
          Fanwarden
        </a>
        <ul class="menu w-full gap-0.5 p-0">
          {#each PAGES as p (p.path)}
            <li>
              <a
                href="#/{p.path}"
                class:menu-active={p === page}
                aria-current={p === page ? 'page' : undefined}
              >
                {p.label}
                {#if p.path === 'alerts' && snapshot?.alerts.length}
                  <span
                    class="badge badge-sm badge-error ml-auto font-figures"
                    aria-label="{snapshot.alerts.length} active">{snapshot.alerts.length}</span
                  >
                {/if}
              </a>
            </li>
          {/each}
        </ul>

        <div class="mt-auto flex flex-col gap-2 text-sm">
          {#if updates.status?.available && updates.status.latest}
            <a class="alert alert-success alert-soft px-3 py-2" href="#/settings"
              >Version {updates.status.latest.version} is available</a
            >
          {/if}
          {#if snapshot?.simulate}<span class="badge badge-warning badge-soft w-full">Simulated hardware</span
            >{/if}
          {#if snapshot?.dry_run}<span class="badge badge-warning badge-soft h-auto w-full py-1"
              >Dry run: speeds are not applied</span
            >{/if}
          <span class="flex items-center gap-2 px-1 text-base-content/60">
            <span class="status {live.connected ? 'status-success' : 'status-error'}"></span>
            {live.connected ? 'Live' : 'Reconnecting…'}
          </span>
          <button class="btn btn-ghost btn-sm justify-start" onclick={logout}>Sign out</button>
        </div>
      </nav>
    </div>
  </div>
{/if}
