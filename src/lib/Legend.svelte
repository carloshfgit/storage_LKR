<!-- Legenda da barra: categorias da maior para a menor, com "Livre" sempre por último. -->
<script lang="ts">
  import { flip } from "svelte/animate";
  import { prefersReducedMotion } from "svelte/motion";
  import type { Category } from "./api";
  import type { Segment } from "./categories";
  import { formatBytes, formatPercent } from "./format";

  let {
    segments,
    free,
    total,
    onselect,
  }: {
    segments: Segment[];
    free: number;
    total: number;
    /** Ausente enquanto não há resultado completo: as linhas não são clicáveis. */
    onselect?: (key: Category) => void;
  } = $props();

  // `sort` é estável: categorias empatadas (ainda em 0) mantêm a ordem da barra.
  const sorted = $derived([...segments].sort((a, b) => b.bytes - a.bytes));
  const share = (bytes: number) => formatPercent(total > 0 ? bytes / total : 0);
</script>

<ul>
  {#each sorted as s (s.key)}
    <li animate:flip={{ duration: prefersReducedMotion.current ? 0 : 300 }}>
      <button class="row" disabled={!onselect} onclick={() => onselect?.(s.key)}>
        <span class="dot" style:background={s.color}></span>
        <span class="name">
          {s.label}
          {#if s.note}<small>{s.note}</small>{/if}
        </span>
        <span class="size">{formatBytes(s.bytes)}</span>
        <span class="share">{share(s.bytes)}</span>
        <svg class="chevron" viewBox="0 0 16 16" aria-hidden="true"><path d="m6 3 5 5-5 5" /></svg>
      </button>
    </li>
  {/each}
  <li>
    <div class="row">
      <span class="dot free"></span>
      <span class="name">Livre</span>
      <span class="size">{formatBytes(free)}</span>
      <span class="share">{share(free)}</span>
    </div>
  </li>
</ul>

<style>
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  li {
    border-top: 1px solid var(--separator);
  }

  li:first-child {
    border-top: none;
  }

  .row {
    display: grid;
    grid-template-columns: 10px 1fr auto 4.5em 14px;
    align-items: center;
    gap: 12px;
    width: 100%;
    padding: 9px 0;
    border: none;
    background: none;
    color: inherit;
    font: inherit;
    text-align: left;
    font-variant-numeric: tabular-nums;
  }

  button.row:enabled {
    cursor: pointer;
  }

  button.row:enabled:hover .name {
    color: var(--accent);
  }

  .chevron {
    width: 14px;
    height: 14px;
    fill: none;
    stroke: var(--text-secondary);
    stroke-width: 1.6;
    stroke-linecap: round;
    stroke-linejoin: round;
    visibility: hidden;
  }

  button.row:enabled .chevron {
    visibility: visible;
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
