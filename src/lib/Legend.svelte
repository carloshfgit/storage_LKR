<!-- Legenda da barra: categorias da maior para a menor, com "Livre" sempre por último. -->
<script lang="ts">
  import { flip } from "svelte/animate";
  import { prefersReducedMotion } from "svelte/motion";
  import type { Segment } from "./categories";
  import { formatBytes, formatPercent } from "./format";

  let { segments, free, total }: { segments: Segment[]; free: number; total: number } = $props();

  // `sort` é estável: categorias empatadas (ainda em 0) mantêm a ordem da barra.
  const sorted = $derived([...segments].sort((a, b) => b.bytes - a.bytes));
  const share = (bytes: number) => formatPercent(total > 0 ? bytes / total : 0);
</script>

<ul>
  {#each sorted as s (s.key)}
    <li animate:flip={{ duration: prefersReducedMotion.current ? 0 : 300 }}>
      <span class="dot" style:background={s.color}></span>
      <span class="name">
        {s.label}
        {#if s.note}<small>{s.note}</small>{/if}
      </span>
      <span class="size">{formatBytes(s.bytes)}</span>
      <span class="share">{share(s.bytes)}</span>
    </li>
  {/each}
  <li>
    <span class="dot free"></span>
    <span class="name">Livre</span>
    <span class="size">{formatBytes(free)}</span>
    <span class="share">{share(free)}</span>
  </li>
</ul>

<style>
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  li {
    display: grid;
    grid-template-columns: 10px 1fr auto 4.5em;
    align-items: center;
    gap: 12px;
    padding: 9px 0;
    border-top: 1px solid var(--separator);
    font-variant-numeric: tabular-nums;
  }

  li:first-child {
    border-top: none;
  }

  .dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
  }

  .dot.free {
    background: var(--track);
    box-shadow: inset 0 0 0 1px var(--separator);
  }

  .name {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  small {
    color: var(--text-secondary);
    font-size: 12px;
  }

  .size {
    font-weight: 500;
  }

  .share {
    color: var(--text-secondary);
    text-align: right;
  }
</style>
