<!-- Um arquivo na lista do detalhe: nome, pasta (com ~ no lugar do $HOME), tamanho e data. -->
<script lang="ts">
  import type { FileEntry } from "./api";
  import { formatBytes, formatRelativeDate } from "./format";

  let {
    file,
    home,
    onreveal,
  }: { file: FileEntry; home: string; onreveal: (path: string) => void } = $props();

  const slash = $derived(file.path.lastIndexOf("/"));
  const name = $derived(file.path.slice(slash + 1));
  const folder = $derived.by(() => {
    const dir = file.path.slice(0, slash) || "/";
    if (home && (dir === home || dir.startsWith(home + "/"))) return "~" + dir.slice(home.length);
    return dir;
  });
  // Nome não UTF-8: chegou com perdas (DECISIONS D26) e o Rust recusaria a ação.
  const lossy = $derived(file.path.includes("�"));
  const date = $derived(new Date(file.modified * 1000).toLocaleString("pt-BR"));
</script>

<li>
  <div class="text">
    <span class="name" title={name}>{name}</span>
    <span class="folder" title={folder}>{folder}</span>
  </div>
  <span class="meta">
    <span class="size">{formatBytes(file.bytes)}</span>
    <span class="date" title={date}>{formatRelativeDate(file.modified)}</span>
  </span>
  <button
    onclick={() => onreveal(file.path)}
    disabled={lossy}
    title={lossy ? "O nome tem caracteres inválidos e não pode ser aberto" : "Abrir na pasta"}
    aria-label="Abrir {name} na pasta"
  >
    <svg viewBox="0 0 16 16" aria-hidden="true">
      <path d="M1.5 4.5v8a1 1 0 0 0 1 1h11a1 1 0 0 0 1-1v-6a1 1 0 0 0-1-1H8L6.5 3.5h-4a1 1 0 0 0-1 1Z" />
    </svg>
  </button>
</li>

<style>
  li {
    display: grid;
    grid-template-columns: 1fr auto 32px;
    align-items: center;
    gap: 12px;
    padding: 8px 0;
    border-top: 1px solid var(--separator);
  }

  li:first-child {
    border-top: none;
  }

  .text,
  .meta {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .name,
  .folder {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .folder,
  .date {
    color: var(--text-secondary);
    font-size: 12px;
  }

  .meta {
    align-items: flex-end;
    font-variant-numeric: tabular-nums;
  }

  .size {
    font-weight: 500;
  }

  button {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    padding: 0;
    border: none;
    border-radius: 8px;
    background: transparent;
    color: var(--accent);
    cursor: pointer;
  }

  button:hover:not(:disabled) {
    background: color-mix(in srgb, var(--accent) 14%, transparent);
  }

  button:disabled {
    color: var(--text-secondary);
    opacity: 0.5;
    cursor: not-allowed;
  }

  svg {
    width: 16px;
    height: 16px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.4;
    stroke-linejoin: round;
  }
</style>
