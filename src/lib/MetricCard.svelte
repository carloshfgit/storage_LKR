<!-- Card da aba Recursos: título, valor grande, subtítulo e o conteúdo (gráfico, listas). -->
<script lang="ts">
  import type { Snippet } from "svelte";

  let {
    title,
    value,
    subtitle,
    badge,
    children,
  }: {
    title: string;
    value: string;
    subtitle?: string;
    /** Selo de alerta ao lado do título, como "Sob pressão". */
    badge?: { text: string; hint: string };
    children?: Snippet;
  } = $props();
</script>

<section class="card" aria-label={title}>
  <header>
    <h2>{title}</h2>
    {#if badge}
      <span class="badge" title={badge.hint}>{badge.text}</span>
    {/if}
  </header>
  <p class="value">{value}</p>
  {#if subtitle}
    <p class="subtitle">{subtitle}</p>
  {/if}
  {@render children?.()}
</section>

<style>
  .card {
    display: flex;
    flex-direction: column;
    gap: 6px;
    min-width: 0;
    padding: 16px 18px;
    background: var(--surface);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
  }

  header {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  h2 {
    margin: 0;
    color: var(--text-secondary);
    font-size: 13px;
    font-weight: 600;
  }

  .badge {
    padding: 1px 8px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--warning) 18%, transparent);
    color: var(--warning);
    font-size: 11px;
    font-weight: 600;
  }

  .value {
    margin: 0;
    font-size: 24px;
    font-weight: 600;
    letter-spacing: -0.01em;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .subtitle {
    margin: -4px 0 4px;
    color: var(--text-secondary);
    font-size: 12px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
