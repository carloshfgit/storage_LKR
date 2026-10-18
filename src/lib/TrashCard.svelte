<!-- Card da Lixeira na tela principal: quanto ela ocupa e quantos itens tem. Abre o detalhe. -->
<script lang="ts">
  import type { ScanResult } from "./api";
  import { formatBytes, formatCount } from "./format";

  let { result, onselect }: { result: ScanResult; onselect: () => void } = $props();

  const items = $derived(result.trash_items.length);
  const empty = $derived(items === 0);
</script>

<section class="card">
  <button onclick={onselect} disabled={empty}>
    <svg class="icon" viewBox="0 0 24 24" aria-hidden="true">
      <path d="M4 7h16M10 11v6M14 11v6M5 7l1 12a2 2 0 0 0 2 2h8a2 2 0 0 0 2-2l1-12M9 7V4h6v3" />
    </svg>
    <span class="text">
      <span class="title">Lixeira</span>
      <span class="detail">
        {#if empty}
          Lixeira vazia
        {:else}
          {formatCount(items)} {items === 1 ? "item" : "itens"} · o espaço só é liberado ao esvaziá-la
        {/if}
      </span>
    </span>
    {#if !empty}
      <span class="size">{formatBytes(result.trash_bytes)}</span>
      <svg class="chevron" viewBox="0 0 16 16" aria-hidden="true"><path d="m6 3 5 5-5 5" /></svg>
    {/if}
  </button>
</section>

<style>
  .card {
    margin-top: 14px;
    padding: 6px 8px;
    background: var(--surface);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
  }

  button {
    display: grid;
    grid-template-columns: 28px 1fr auto 16px;
    align-items: center;
    gap: 10px;
    width: 100%;
    padding: 9px 12px;
    border: none;
    border-radius: 10px;
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
    transition: background var(--duration) var(--ease);
  }

  button:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent) 8%, transparent);
  }

  button:disabled {
    cursor: default;
  }

  .text {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .title {
    font-size: 15px;
    font-weight: 600;
  }

  .detail {
    color: var(--text-secondary);
    font-size: 12px;
  }

  .size {
    font-weight: 500;
    font-variant-numeric: tabular-nums;
  }

  svg {
    fill: none;
    stroke-width: 1.8;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  .icon {
    width: 24px;
    height: 24px;
    stroke: var(--text-secondary);
  }

  .chevron {
    width: 14px;
    height: 14px;
    stroke: var(--text-secondary);
  }
</style>
