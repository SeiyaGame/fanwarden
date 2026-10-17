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

<div class="optional">
  <label class="check" for={id}>
    <input
      {id}
      type="checkbox"
      checked={value !== null}
      onchange={(e) => (value = e.currentTarget.checked ? fallback : null)}
    />
    {label}
  </label>
  {#if value !== null}
    <label class="amount"
      ><input type="number" bind:value aria-label="{label} ({unit})" />
      <span class="soft">{unit}</span></label
    >
  {/if}
</div>

<style>
  .optional {
    display: grid;
    gap: 0.35rem;
    align-content: start;
  }

  .amount {
    display: flex;
    align-items: center;
    gap: 0.4rem;
    padding-left: 1.6rem;
  }

  .amount input {
    width: 6rem;
  }
</style>
