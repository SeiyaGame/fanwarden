<script lang="ts">
  // A threshold that can be switched off (stored as null).
  let {
    value = $bindable(),
    label,
    unit,
    fallback,
  }: { value: number | null; label: string; unit: string; fallback: number } = $props()

  const id = `opt-${Math.random().toString(36).slice(2, 9)}`
</script>

<div class="grid content-start gap-2">
  <label class="label cursor-pointer gap-2 text-base-content" for={id}>
    <input
      {id}
      type="checkbox"
      class="toggle toggle-sm toggle-primary"
      checked={value !== null}
      onchange={(e) => (value = e.currentTarget.checked ? fallback : null)}
    />
    {label}
  </label>
  {#if value !== null}
    <label class="input input-sm ml-10 w-36">
      <input type="number" bind:value aria-label="{label} ({unit})" />
      <span class="label">{unit}</span>
    </label>
  {/if}
</div>
