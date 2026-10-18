<!-- Sugestões de limpeza na tela principal: downloads antigos, caches e maiores arquivos. -->
<script module lang="ts">
  export type SuggestionKind = "old_downloads" | "caches" | "largest";

  /** Quantos arquivos o card "Maiores arquivos" mostra. */
  export const LARGEST_COUNT = 10;
</script>

<script lang="ts">
  import { largestFiles, OLD_DOWNLOAD_DAYS, type FileItem, type ScanResult } from "./api";
  import { formatBytes, formatCount } from "./format";

  let { result, onselect }: { result: ScanResult; onselect: (kind: SuggestionKind) => void } = $props();

  /** Os maiores vêm do Rust, que já descarta os que não podem ir para a Lixeira. */
  let largest = $state.raw<FileItem[]>([]);

  $effect(() => {
    const current = result;
    largestFiles(LARGEST_COUNT).then(
      (list) => current === result && (largest = list),
      () => (largest = []),
    );
  });

  const plural = (n: number, one: string, many: string) => `${formatCount(n)} ${n === 1 ? one : many}`;

  const rows = $derived.by(() => {
    const s = result.suggestions;
    const list: { kind: SuggestionKind; title: string; detail: string; bytes: number }[] = [];
    if (s.old_downloads.files > 0) {
      list.push({
        kind: "old_downloads",
        title: "Downloads antigos",
        detail: `${plural(s.old_downloads.files, "arquivo", "arquivos")} sem uso há mais de ${OLD_DOWNLOAD_DAYS} dias`,
        bytes: s.old_downloads.bytes,
      });
    }
    if (s.caches.length > 0) {
      list.push({
        kind: "caches",
        title: "Caches",
        detail: `${plural(s.caches.length, "pasta", "pastas")} em ~/.cache que os apps recriam`,
        bytes: s.caches.reduce((sum, c) => sum + c.bytes, 0),
      });
    }
    if (largest.length > 0) {
      list.push({
        kind: "largest",
        title: "Maiores arquivos",
        detail: `Os ${formatCount(largest.length)} maiores da pasta pessoal`,
        bytes: largest.reduce((sum, f) => sum + f.bytes, 0),
      });
    }
    return list;
  });
</script>

{#if rows.length > 0}
  <section class="card">
    <h2>Sugestões</h2>
    <ul>
      {#each rows as row (row.kind)}
        <li>
          <button onclick={() => onselect(row.kind)}>
            <span class="text">
              <span class="title">{row.title}</span>
              <span class="detail">{row.detail}</span>
            </span>
            <span class="size">{formatBytes(row.bytes)}</span>
            <svg viewBox="0 0 16 16" aria-hidden="true"><path d="m6 3 5 5-5 5" /></svg>
          </button>
        </li>
      {/each}
    </ul>
  </section>
{/if}

<style>
  .card {
    margin-top: 14px;
    padding: 14px 8px 6px;
    background: var(--surface);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
  }

  h2 {
    margin: 0 12px 4px;
    font-size: 15px;
    font-weight: 600;
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  li + li button {
    border-top: 1px solid var(--separator);
  }

  button {
    display: grid;
    grid-template-columns: 1fr auto 16px;
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

  li + li button:hover,
  button:hover {
    border-color: transparent;
    background: color-mix(in srgb, var(--accent) 8%, transparent);
  }

  .text {
    display: flex;
    flex-direction: column;
    min-width: 0;
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
    width: 14px;
    height: 14px;
    fill: none;
    stroke: var(--text-secondary);
    stroke-width: 1.8;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
</style>
