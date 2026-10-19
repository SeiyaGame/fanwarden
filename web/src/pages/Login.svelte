<script lang="ts">
  import Rotor from '../components/Rotor.svelte'
  import { api } from '../lib/api'

  let { setup, onDone }: { setup: boolean; onDone: () => void } = $props()

  let password = $state('')
  let confirm = $state('')
  let error = $state<string | null>(null)
  let busy = $state(false)

  async function submit(event: SubmitEvent) {
    event.preventDefault()
    error = null
    if (setup && password !== confirm) {
      error = 'The two passwords are different.'
      return
    }
    busy = true
    try {
      await (setup ? api.setup(password) : api.login(password))
      onDone()
    } catch (e) {
      error = e instanceof Error ? e.message : String(e)
    } finally {
      busy = false
    }
  }
</script>

<main class="grid min-h-dvh place-items-center bg-base-200 p-4">
  <form class="card w-full max-w-sm border border-base-300 bg-base-100" onsubmit={submit}>
    <div class="card-body gap-4">
      <div class="flex items-center gap-3">
        <Rotor rpm={setup ? 900 : 500} size={44} color="var(--color-accent)" />
        <h1 class="text-2xl font-semibold">Fanwarden</h1>
      </div>
      {#if setup}
        <p class="text-base-content/70">
          Choose the password that protects this page. It needs at least 8 characters.
        </p>
      {/if}
      <fieldset class="fieldset">
        <legend class="fieldset-legend">Password</legend>
        <!-- svelte-ignore a11y_autofocus -->
        <input
          class="input w-full"
          type="password"
          bind:value={password}
          autocomplete={setup ? 'new-password' : 'current-password'}
          minlength={setup ? 8 : undefined}
          aria-label="Password"
          required
          autofocus
        />
        {#if setup}
          <legend class="fieldset-legend">Password again</legend>
          <input
            class="input w-full"
            type="password"
            bind:value={confirm}
            autocomplete="new-password"
            aria-label="Password again"
            required
          />
        {/if}
      </fieldset>
      {#if error}<div role="alert" class="alert alert-error alert-soft">{error}</div>{/if}
      <button class="btn btn-primary" type="submit" disabled={busy}>
        {#if busy}<span class="loading loading-spinner loading-sm"></span>{/if}
        {setup ? 'Create password' : 'Sign in'}
      </button>
    </div>
  </form>
</main>
