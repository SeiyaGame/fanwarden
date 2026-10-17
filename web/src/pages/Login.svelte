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

<main class="login">
  <form class="panel" onsubmit={submit}>
    <div class="brand">
      <Rotor rpm={setup ? 900 : 500} size={44} color="var(--warm)" />
      <h1>Fan control</h1>
    </div>
    {#if setup}
      <p class="soft">Choose the password that protects this page. It needs at least 8 characters.</p>
    {/if}
    <label class="field"
      >Password
      <!-- svelte-ignore a11y_autofocus -->
      <input
        type="password"
        bind:value={password}
        autocomplete={setup ? 'new-password' : 'current-password'}
        minlength={setup ? 8 : undefined}
        required
        autofocus
      /></label
    >
    {#if setup}
      <label class="field"
        >Password again <input
          type="password"
          bind:value={confirm}
          autocomplete="new-password"
          required
        /></label
      >
    {/if}
    {#if error}<p class="error" role="alert">{error}</p>{/if}
    <button type="submit" disabled={busy}>{setup ? 'Create password' : 'Sign in'}</button>
  </form>
</main>

<style>
  .login {
    min-height: 100dvh;
    display: grid;
    place-items: center;
    padding: var(--gutter);
  }

  form {
    width: min(100%, 360px);
    display: grid;
    gap: 1rem;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 0.75rem;
  }

  .error {
    color: var(--critical);
  }
</style>
