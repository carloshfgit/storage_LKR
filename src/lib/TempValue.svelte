<!-- Temperatura de um sensor, em destaque acima do limiar dele. Sem sensor, "—". -->
<script lang="ts">
  import type { Temp } from "./api";
  import { formatTemp } from "./format";

  let { temp }: { temp: Temp | null } = $props();

  const hot = $derived(temp !== null && temp.celsius >= temp.hot_c);
</script>

{#if temp}
  <span
    class="temp"
    class:hot
    title={temp.approximate
      ? "Sensor da placa-mãe: indica a temperatura perto da CPU, não a do chip"
      : hot
        ? `Acima de ${formatTemp(temp.hot_c)}`
        : undefined}
  >
    {temp.approximate ? "~" : ""}{formatTemp(temp.celsius)}
  </span>
{:else}
  <span class="temp none" title="Sem sensor de temperatura">—</span>
{/if}

<style>
  .temp {
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .hot {
    color: var(--danger);
    font-weight: 600;
  }

  .none {
    color: var(--text-secondary);
  }
</style>
