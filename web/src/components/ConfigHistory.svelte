<script lang="ts">
  // Last versions of the configuration: what changed in each, and a way back to any of them.
  import { api } from '../lib/api'
  import { configStore } from '../lib/configStore.svelte'
  import { countChanges, diffLines, withContext } from '../lib/diff'
  import type { ConfigVersion } from '../lib/types'

  let versions = $state<ConfigVersion[] | null>(null)
  let openId = $state<number | null>(null)
  let confirmId = $state<number | null>(null)
  let busy = $state(false)
  let error = $state<string | null>(null)

  async function load() {
    try {
      versions = await api.configHistory()
      error = null
    } catch (e) {
      error = e instanceof Error ? e.message : String(e)
    }
  }

  $effect(() => {
    load()
  })

  /** Changes of a version against the one before it (versions are newest first). */
  function changes(index: number) {
    if (!versions) return []
    const previous = versions[index + 1]
    return previous ? diffLines(previous.toml, versions[index].toml) : []
  }

  async function restore(version: ConfigVersion) {
    busy = true
    error = null
    try {
      await api.restoreConfig(version.id)
      await configStore.load()
      confirmId = null
      await load()
      openId = versions?.[0]?.id ?? null
    } catch (e) {
      error = e instanceof Error ? e.message : String(e)
    } finally {
      busy = false
    }
  }

  const date = (ts: number) => new Date(ts * 1000).toLocaleString()

  function originText(version: ConfigVersion): string {
    switch (version.origin.kind) {
      case 'startup':
        return 'Found at startup'
      case 'ui':
        return 'Saved from the web UI'
      case 'file':
        return 'Reloaded from config.toml'
      case 'restore':
        return `Back to the version of ${date(version.origin.from_ts)}`
    }
  }
</script>

<section class="card border border-base-300 bg-base-100" aria-labelledby="config-history-title">
  <div class="card-body gap-4">
    <header class="grid gap-1">
      <h2 id="config-history-title" class="card-title">Configuration changes</h2>
      <p class="text-sm text-base-content/70">
        The last 20 versions. Secrets are never shown: a changed password or token appears as a different
        <code class="font-mono">(secret n)</code>.
      </p>
    </header>

    {#if error}<div role="alert" class="alert alert-error alert-soft">{error}</div>{/if}

    {#if versions === null}
      <p class="flex items-center gap-2 text-base-content/60">
        <span class="loading loading-spinner loading-sm"></span> Loading…
      </p>
    {:else}
      <ol class="grid">
        {#each versions as version, index (version.id)}
          {@const lines = changes(index)}
          {@const counts = countChanges(lines)}
          {@const isCurrent = index === 0}
          <li class="border-t border-base-300">
            <button
              class="grid w-full cursor-pointer grid-cols-[1fr_auto] items-baseline gap-x-4 gap-y-0.5 rounded px-2 py-2.5 text-left hover:bg-base-200 focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-primary sm:grid-cols-[11.5rem_1fr_auto_auto] {openId ===
              version.id
                ? 'bg-base-200'
                : ''}"
              aria-expanded={openId === version.id}
              onclick={() => {
                openId = openId === version.id ? null : version.id
                confirmId = null
              }}
            >
              <time class="font-figures font-medium tabular-nums">{date(version.ts)}</time>
              <span>{originText(version)}</span>
              <span class="font-figures tabular-nums">
                {#if index === versions.length - 1}
                  <span class="text-base-content/60">oldest kept</span>
                {:else}
                  <span class="text-success">+{counts.added}</span>
                  <span class="text-error">−{counts.removed}</span>
                {/if}
              </span>
              {#if isCurrent}<span class="badge badge-sm badge-primary">current</span>{/if}
            </button>

            {#if openId === version.id}
              <div class="grid gap-3 px-2 pt-2 pb-4">
                {#if index < versions.length - 1}
                  <div class="overflow-x-auto rounded-box bg-base-200 py-2 font-mono text-sm leading-normal">
                    {#each withContext(lines) as row, r (r)}
                      {#if row.kind === 'fold'}
                        <span class="block px-3 text-base-content/60 italic"
                          >… {row.count} unchanged lines</span
                        >
                      {:else}
                        <span
                          class="block px-3 whitespace-pre {row.kind === 'add'
                            ? 'bg-success/20'
                            : row.kind === 'del'
                              ? 'bg-error/20'
                              : ''}"
                          >{row.kind === 'add' ? '+ ' : row.kind === 'del' ? '- ' : '  '}{row.text}</span
                        >
                      {/if}
                    {/each}
                  </div>
                {:else}
                  <p class="text-base-content/60">The oldest version kept: nothing to compare it with.</p>
                {/if}

                {#if !isCurrent}
                  <div class="flex flex-wrap items-center gap-2">
                    {#if confirmId === version.id}
                      <div role="alert" class="alert alert-warning alert-soft w-full">
                        <span
                          >Go back to this version?{configStore.dirty
                            ? ' Your unsaved changes will be lost.'
                            : ''}</span
                        >
                        <div class="flex gap-2">
                          <button class="btn btn-ghost btn-sm" onclick={() => (confirmId = null)}
                            >Cancel</button
                          >
                          <button
                            class="btn btn-warning btn-sm"
                            disabled={busy}
                            onclick={() => restore(version)}
                          >
                            {#if busy}<span class="loading loading-spinner loading-xs"></span>{/if}
                            Yes, go back
                          </button>
                        </div>
                      </div>
                    {:else}
                      <button class="btn btn-sm" onclick={() => (confirmId = version.id)}
                        >Go back to this version</button
                      >
                    {/if}
                  </div>
                {/if}
              </div>
            {/if}
          </li>
        {:else}
          <li class="text-base-content/60">No version recorded yet.</li>
        {/each}
      </ol>
    {/if}
  </div>
</section>
