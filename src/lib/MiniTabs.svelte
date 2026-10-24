<!-- Botões pequenos de escolha no título de um card da aba Recursos: as placas do card GPU, o critério
     do card Processos. -->
<script lang="ts" generics="T extends string">
  let {
    options,
    value,
    label,
    onchange,
  }: {
    options: { id: T; label: string; title?: string }[];
    value: T;
    /** Nome do grupo para o leitor de tela. */
    label: string;
    onchange: (id: T) => void;
  } = $props();
</script>

<div class="tabs" role="group" aria-label={label}>
  {#each options as o (o.id)}
    <button aria-pressed={o.id === value} title={o.title} onclick={() => onchange(o.id)}>{o.label}</button>
  {/each}
</div>

<style>
  .tabs {
    display: flex;
    gap: 2px;
    padding: 2px;
    border-radius: 7px;
    background: var(--track);
  }

  button {
    padding: 2px 8px;
    border: none;
    border-radius: 5px;
    background: transparent;
    color: var(--text-secondary);
    font: inherit;
    font-size: 11px;
    font-weight: 600;
    white-space: nowrap;
    cursor: pointer;
  }

  button[aria-pressed="true"] {
    background: var(--surface);
    color: var(--text);
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.12);
  }

  button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }

  @media (prefers-color-scheme: dark) {
    button[aria-pressed="true"] {
      background: #636366;
    }
  }
</style>
