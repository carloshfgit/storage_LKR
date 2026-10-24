<!-- Controle segmentado do topo (estilo iOS): Armazenamento | Recursos. Setas, Home e End trocam de aba. -->
<script lang="ts" module>
  export type Tab = "storage" | "resources";
</script>

<script lang="ts">
  let { value, onchange }: { value: Tab; onchange: (tab: Tab) => void } = $props();

  const TABS: { id: Tab; label: string }[] = [
    { id: "storage", label: "Armazenamento" },
    { id: "resources", label: "Recursos" },
  ];

  const index = $derived(TABS.findIndex((t) => t.id === value));
  let buttons = $state<HTMLButtonElement[]>([]);

  function onkeydown(e: KeyboardEvent) {
    const last = TABS.length - 1;
    const next =
      e.key === "ArrowRight" ? Math.min(index + 1, last)
      : e.key === "ArrowLeft" ? Math.max(index - 1, 0)
      : e.key === "Home" ? 0
      : e.key === "End" ? last
      : -1;
    if (next < 0) return;
    e.preventDefault();
    onchange(TABS[next].id);
    buttons[next]?.focus();
  }
</script>

<div class="switch" role="tablist" aria-label="Seção" style:--index={index} style:--count={TABS.length}>
  <span class="thumb" aria-hidden="true"></span>
  {#each TABS as tab, i (tab.id)}
    <button
      bind:this={buttons[i]}
      role="tab"
      id="tab-{tab.id}"
      aria-selected={tab.id === value}
      aria-controls="panel-{tab.id}"
      tabindex={tab.id === value ? 0 : -1}
      onclick={() => onchange(tab.id)}
      {onkeydown}
    >
      {tab.label}
    </button>
  {/each}
</div>

<style>
  .switch {
    position: relative;
    display: grid;
    grid-template-columns: repeat(var(--count), 1fr);
    width: min(100%, 340px);
    margin: 0 auto 20px;
    padding: 2px;
    border-radius: 9px;
    background: var(--track);
  }

  /* O fundo branco desliza para a aba escolhida. */
  .thumb {
    position: absolute;
    top: 2px;
    bottom: 2px;
    left: 2px;
    width: calc((100% - 4px) / var(--count));
    border-radius: 7px;
    background: var(--surface);
    box-shadow:
      0 3px 8px rgba(0, 0, 0, 0.12),
      0 3px 1px rgba(0, 0, 0, 0.04);
    transform: translateX(calc(100% * var(--index)));
    transition: transform 0.28s var(--ease);
  }

  button {
    position: relative;
    padding: 6px 12px;
    border: none;
    border-radius: 7px;
    background: transparent;
    color: var(--text);
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
  }

  button[aria-selected="true"] {
    font-weight: 600;
  }

  button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }

  @media (prefers-color-scheme: dark) {
    .thumb {
      background: #636366;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .thumb {
      transition: none;
    }
  }
</style>
