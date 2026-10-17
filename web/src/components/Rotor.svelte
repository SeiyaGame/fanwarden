<script lang="ts">
  // A fan rotor that spins at a speed derived from the real RPM. Still when there is no reading.
  let {
    rpm = null,
    size = 48,
    color = 'currentColor',
  }: { rpm?: number | null; size?: number; color?: string } = $props()

  // Real fans spin too fast to see; map 300–3000 RPM onto 4s–0.35s per turn.
  const period = $derived(rpm && rpm > 0 ? Math.min(4, Math.max(0.35, 1200 / rpm)) : 0)
</script>

<svg
  class="rotor"
  class:still={!period}
  width={size}
  height={size}
  viewBox="-12 -12 24 24"
  style:--period="{period}s"
  aria-hidden="true"
>
  <circle r="11.4" fill="none" stroke="var(--rule)" stroke-width="0.8" />
  <g class="blades" fill={color}>
    {#each [0, 72, 144, 216, 288] as angle (angle)}
      <path d="M0-2.2C1.6-6 4.6-8.6 7.4-8.2 6.2-5 3.6-2.6.9-1.6z" transform="rotate({angle})" />
    {/each}
    <circle r="2.3" />
  </g>
</svg>

<style>
  .rotor {
    flex: none;
    display: block;
  }

  .blades {
    animation: spin var(--period) linear infinite;
    transform-origin: 0 0;
  }

  .still .blades {
    animation: none;
    opacity: 0.35;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
