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

<section class="panel" aria-labelledby="config-history-title">
  <header>
    <h2 id="config-history-title">Configuration changes</h2>
    <p class="soft">
      The last 20 versions. Secrets are never shown: a changed password or token appears as a different
      <code>(secret n)</code>.
    </p>
  </header>

  {#if error}<p class="fault">{error}</p>{/if}

  {#if versions === null}
    <p class="soft">Loading…</p>
  {:else}
    <ol class="versions">
      {#each versions as version, index (version.id)}
        {@const lines = changes(index)}
        {@const counts = countChanges(lines)}
        {@const isCurrent = index === 0}
        <li class:open={openId === version.id}>
          <button
            class="summary"
            aria-expanded={openId === version.id}
            onclick={() => {
              openId = openId === version.id ? null : version.id
              confirmId = null
            }}
          >
            <time class="figure">{date(version.ts)}</time>
            <span>{originText(version)}</span>
            <span class="counts figure">
              {#if index === versions.length - 1}
                <span class="soft">oldest kept</span>
              {:else}
                <span class="added">+{counts.added}</span> <span class="removed">−{counts.removed}</span>
              {/if}
            </span>
            {#if isCurrent}<span class="current">current</span>{/if}
          </button>

          {#if openId === version.id}
            <div class="detail">
              {#if index < versions.length - 1}
                <div class="diff">
                  {#each withContext(lines) as row, r (r)}
                    {#if row.kind === 'fold'}
                      <span class="fold">… {row.count} unchanged lines</span>
                    {:else}
                      <span class={row.kind}
                        >{row.kind === 'add' ? '+ ' : row.kind === 'del' ? '- ' : '  '}{row.text}</span
                      >
                    {/if}
                  {/each}
                </div>
              {:else}
                <p class="soft">The oldest version kept: nothing to compare it with.</p>
              {/if}

              {#if !isCurrent}
                <div class="actions">
                  {#if confirmId === version.id}
                    <span
                      >Go back to this version?{configStore.dirty
                        ? ' Your unsaved changes will be lost.'
                        : ''}</span
                    >
                    <button disabled={busy} onclick={() => restore(version)}>Yes, go back</button>
                    <button class="quiet" onclick={() => (confirmId = null)}>Cancel</button>
                  {:else}
                    <button class="quiet" onclick={() => (confirmId = version.id)}
                      >Go back to this version</button
                    >
                  {/if}
                </div>
              {/if}
            </div>
          {/if}
        </li>
      {:else}
        <li class="soft">No version recorded yet.</li>
      {/each}
    </ol>
  {/if}
</section>

<style>
  section {
    display: grid;
    gap: 1rem;
  }

  header {
    display: grid;
    gap: 0.35rem;
  }

  header p {
    font-size: var(--step--1);
    margin: 0;
  }

  .versions {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
  }

  .versions > li {
    border-top: 1px solid var(--rule);
  }

  .summary {
    all: unset;
    box-sizing: border-box;
    width: 100%;
    display: grid;
    grid-template-columns: 11.5rem 1fr auto auto;
    gap: 1rem;
    align-items: baseline;
    padding: 0.55rem 0.4rem;
    cursor: pointer;
  }

  .summary:hover,
  .open .summary {
    background: var(--panel-sunk);
  }

  .summary:focus-visible {
    outline: 2px solid var(--focus);
    outline-offset: -2px;
  }

  time {
    font-weight: 500;
  }

  .added {
    color: var(--mild);
  }

  .removed {
    color: var(--hot);
  }

  .current {
    font-size: var(--step--1);
    font-weight: 600;
    border: 1px solid var(--rule);
    border-radius: var(--radius-s);
    padding: 0 0.4rem;
  }

  .detail {
    display: grid;
    gap: 0.75rem;
    padding: 0.5rem 0.4rem 1rem;
  }

  .diff {
    font-family: ui-monospace, 'Cascadia Mono', Consolas, monospace;
    padding: 0.6rem 0;
    background: var(--panel-sunk);
    border-radius: var(--radius-s);
    overflow-x: auto;
    font-size: var(--step--1);
    line-height: 1.5;
  }

  .diff span {
    display: block;
    padding: 0 0.75rem;
    white-space: pre;
  }

  .diff .add {
    background: color-mix(in srgb, var(--mild) 22%, transparent);
  }

  .diff .del {
    background: color-mix(in srgb, var(--hot) 20%, transparent);
  }

  .diff .fold {
    color: var(--ink-soft);
    font-style: italic;
  }

  .actions {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 0.5rem 0.75rem;
  }

  .fault {
    color: var(--critical);
  }

  @media (max-width: 560px) {
    .summary {
      grid-template-columns: 1fr auto;
      gap: 0.2rem 1rem;
    }
  }
</style>
