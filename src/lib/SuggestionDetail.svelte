<!-- Detalhe de uma sugestão: downloads antigos e maiores arquivos (arquivos, como no
     detalhe da categoria) ou caches (itens inteiros de ~/.cache). -->
<script lang="ts">
  import { homeDir } from "@tauri-apps/api/path";
  import { untrack } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import {
    largestFiles,
    moveToTrash,
    OLD_DOWNLOAD_DAYS,
    oldDownloads,
    reveal,
    trashCaches,
    type FileItem,
    type ScanResult,
    type TrashReport,
  } from "./api";
  import ConfirmDialog, { type ConfirmRequest } from "./ConfirmDialog.svelte";
  import { formatBytes, formatCount } from "./format";
  import FileRow from "./FileRow.svelte";
  import { LARGEST_COUNT, type SuggestionKind } from "./Suggestions.svelte";

  let {
    kind,
    result,
    version,
    onback,
    onupdate,
  }: {
    kind: SuggestionKind;
    result: ScanResult;
    /** Muda a cada scan concluído: a lista é recarregada do resultado novo. */
    version: number;
    onback: () => void;
    /** Recebe o resultado atualizado pelo Rust depois de cada ação. */
    onupdate: (result: ScanResult) => void;
  } = $props();

  let files = $state.raw<FileItem[] | null>(null);
  let loadError = $state("");
  let home = $state("");
  /** Incrementado depois de uma ação para recarregar a lista. */
  let reload = $state(0);
  const selection = new SvelteSet<string>();
  let busy = $state(false);
  let notice = $state<{ kind: "ok" | "error"; text: string; details?: string[] } | null>(null);
  let confirm = $state<(ConfirmRequest & { run: () => void }) | null>(null);

  const caches = $derived(result.suggestions.caches);
  const isCaches = $derived(kind === "caches");

  $effect(() => {
    const requested = kind;
    reload;
    version;
    untrack(() => selection.clear());
    files = null;
    loadError = "";
    if (requested === "caches") return;
    const load = requested === "old_downloads" ? oldDownloads() : largestFiles(LARGEST_COUNT);
    load.then(
      (list) => requested === kind && (files = list),
      (err) => (loadError = String(err)),
    );
  });

  homeDir().then(
    (dir) => (home = dir.replace(/\/$/, "")),
    () => {},
  );

  const titles: Record<SuggestionKind, string> = {
    old_downloads: "Downloads antigos",
    caches: "Caches",
    largest: "Maiores arquivos",
  };

  const plural = (n: number, one: string, many: string) => `${formatCount(n)} ${n === 1 ? one : many}`;
  const fileName = (path: string) => path.slice(path.lastIndexOf("/") + 1);
  const tilde = (path: string) => (home && path.startsWith(home + "/") ? "~" + path.slice(home.length) : path);

  const summary = $derived.by(() => {
    const s = result.suggestions;
    if (kind === "old_downloads") {
      const dir = s.downloads_dir ? tilde(s.downloads_dir) : "Downloads";
      return {
        bytes: s.old_downloads.bytes,
        text: `${plural(s.old_downloads.files, "arquivo", "arquivos")} em ${dir} sem modificação há mais de ${OLD_DOWNLOAD_DAYS} dias`,
      };
    }
    if (kind === "caches") {
      return {
        bytes: caches.reduce((sum, c) => sum + c.bytes, 0),
        text: `${plural(caches.length, "pasta", "pastas")} em ~/.cache`,
      };
    }
    return {
      bytes: (files ?? []).reduce((sum, f) => sum + f.bytes, 0),
      text: "Os maiores arquivos da pasta pessoal que podem ir para a Lixeira",
    };
  });

  const selectedBytes = $derived(
    isCaches
      ? caches.reduce((sum, c) => sum + (selection.has(c.path) ? c.bytes : 0), 0)
      : (files ?? []).reduce((sum, f) => sum + (selection.has(f.path) ? f.bytes : 0), 0),
  );

  function toggle(path: string) {
    if (!selection.delete(path)) selection.add(path);
  }

  async function onreveal(path: string) {
    try {
      await reveal(path);
    } catch (err) {
      notice = { kind: "error", text: `Não foi possível abrir a pasta: ${err}` };
    }
  }

  async function run(action: () => Promise<TrashReport>, nouns: [string, string]) {
    busy = true;
    notice = null;
    try {
      const report = await action();
      onupdate(report.result);
      const parts: string[] = [];
      if (report.moved > 0) {
        parts.push(
          `${plural(report.moved, ...nouns)} para a Lixeira (${formatBytes(report.moved_bytes)}).` +
            " Dá para restaurar pela Lixeira do Nautilus; o espaço só é liberado ao esvaziá-la.",
        );
      }
      if (report.rejected.length > 0) {
        parts.push(`${plural(report.rejected.length, "item recusado", "itens recusados")}:`);
      }
      notice = {
        kind: report.rejected.length > 0 && report.moved === 0 ? "error" : "ok",
        text: parts.join(" "),
        details: report.rejected.map((r) => `${fileName(r.path)}: ${r.reason}`),
      };
      reload++;
    } catch (err) {
      notice = { kind: "error", text: `Não foi possível mover para a Lixeira: ${err}` };
    } finally {
      busy = false;
    }
  }

  function moveSelected() {
    const paths = [...selection];
    if (!isCaches) {
      run(() => moveToTrash(paths), ["arquivo movido", "arquivos movidos"]);
      return;
    }
    // Pastas inteiras: pede confirmação, mesmo sendo restaurável.
    confirm = {
      title: `Mover ${plural(paths.length, "pasta", "pastas")} de cache (${formatBytes(selectedBytes)}) para a Lixeira?`,
      message:
        "Os apps recriam o cache quando precisarem, mas podem ficar mais lentos na próxima abertura e pedir login de novo. Feche-os antes.",
      confirmLabel: "Mover para a Lixeira",
      run: () => run(() => trashCaches(paths), ["pasta movida", "pastas movidas"]),
    };
  }

  function onconfirm() {
    const action = confirm?.run;
    confirm = null;
    action?.();
  }
</script>

<!-- Com o diálogo aberto, o Esc é dele: fecha o diálogo, não o detalhe. -->
<svelte:window onkeydown={(e) => e.key === "Escape" && !confirm && onback()} />

<button class="back" onclick={onback}>
  <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M10 3 5 8l5 5" /></svg>
  Armazenamento
</button>

<header>
  <h1>{titles[kind]}</h1>
  <p><strong>{formatBytes(summary.bytes)}</strong> · {summary.text}</p>
</header>

{#if notice}
  <div class="notice {notice.kind}" role="status">
    <p>{notice.text}</p>
    {#if notice.details?.length}
      <ul>
        {#each notice.details as line}<li>{line}</li>{/each}
      </ul>
    {/if}
    <button class="dismiss" onclick={() => (notice = null)} aria-label="Fechar aviso">×</button>
  </div>
{/if}

<section class="card">
  {#if isCaches}
    {#if caches.length === 0}
      <p class="muted">Nenhum cache encontrado.</p>
    {:else}
      <ul class="caches">
        {#each caches as cache (cache.path)}
          {@const lossy = cache.path.includes("�")}
          <li>
            <input
              type="checkbox"
              checked={selection.has(cache.path)}
              disabled={busy || lossy}
              onchange={() => toggle(cache.path)}
              aria-label="Selecionar {fileName(cache.path)}"
            />
            <div class="text">
              <span class="name">{fileName(cache.path)}</span>
              <span class="folder">{tilde(cache.path)} · {plural(cache.files, "arquivo", "arquivos")}</span>
            </div>
            <span class="size">{formatBytes(cache.bytes)}</span>
            <button
              class="reveal"
              onclick={() => onreveal(cache.path)}
              disabled={lossy}
              title="Abrir na pasta"
              aria-label="Abrir {fileName(cache.path)} na pasta"
            >
              <svg viewBox="0 0 16 16" aria-hidden="true">
                <path d="M1.5 4.5v8a1 1 0 0 0 1 1h11a1 1 0 0 0 1-1v-6a1 1 0 0 0-1-1H8L6.5 3.5h-4a1 1 0 0 0-1 1Z" />
              </svg>
            </button>
          </li>
        {/each}
      </ul>
      <p class="footnote">
        As {formatCount(caches.length)} maiores pastas de ~/.cache. Cada uma vai inteira para a Lixeira.
      </p>
    {/if}
  {:else if loadError}
    <p class="error">Erro ao carregar os arquivos: {loadError}</p>
  {:else if !files}
    <p class="muted">Carregando…</p>
  {:else if files.length === 0}
    <p class="muted">Nada a sugerir aqui.</p>
  {:else}
    <ul>
      {#each files as file (file.path)}
        <FileRow
          {file}
          {home}
          selected={selection.has(file.path)}
          disabled={busy}
          ontoggle={() => toggle(file.path)}
          {onreveal}
        />
      {/each}
    </ul>
    {#if kind === "old_downloads" && files.length < result.suggestions.old_downloads.files}
      <p class="footnote">Os {formatCount(files.length)} maiores.</p>
    {/if}
  {/if}
</section>

{#if selection.size > 0}
  <div class="selection-bar">
    <span>
      {plural(selection.size, "selecionado", "selecionados")} · <strong>{formatBytes(selectedBytes)}</strong>
    </span>
    <button class="link" onclick={() => selection.clear()} disabled={busy}>Limpar seleção</button>
    <button class="primary" onclick={moveSelected} disabled={busy}>
      {busy ? "Movendo…" : "Mover para a Lixeira"}
    </button>
  </div>
{/if}

<ConfirmDialog request={confirm} {onconfirm} oncancel={() => (confirm = null)} />

<style>
  .back {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    margin: 0 0 8px -6px;
    padding: 4px 8px 4px 2px;
    border: none;
    border-radius: 8px;
    background: transparent;
    color: var(--accent);
    font-size: 15px;
    cursor: pointer;
  }

  .back:hover {
    background: color-mix(in srgb, var(--accent) 12%, transparent);
  }

  .back svg,
  .reveal svg {
    width: 18px;
    height: 18px;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.8;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  header {
    margin-bottom: 16px;
  }

  h1 {
    margin: 0;
    font-size: 28px;
    font-weight: 700;
  }

  header p {
    margin: 2px 0 0;
    color: var(--text-secondary);
    font-variant-numeric: tabular-nums;
  }

  header strong {
    color: var(--text);
    font-weight: 600;
  }

  .card {
    margin-bottom: 14px;
    padding: 14px 20px;
    background: var(--surface);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
  }

  .muted,
  .footnote {
    color: var(--text-secondary);
    font-size: 13px;
  }

  .footnote {
    margin: 8px 0 0;
  }

  .error {
    color: var(--danger);
    font-size: 13px;
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  /* Mesma grade do FileRow: checkbox, texto, tamanho e "abrir na pasta". */
  .caches li {
    display: grid;
    grid-template-columns: 18px 1fr auto 32px;
    align-items: center;
    gap: 10px;
    padding: 8px 0;
    border-top: 1px solid var(--separator);
  }

  .caches li:first-child {
    border-top: none;
  }

  .text {
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

  .folder {
    color: var(--text-secondary);
    font-size: 12px;
  }

  .size {
    font-weight: 500;
    font-variant-numeric: tabular-nums;
  }

  .reveal {
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

  .reveal:hover {
    background: color-mix(in srgb, var(--accent) 12%, transparent);
  }

  .notice {
    position: relative;
    margin-bottom: 14px;
    padding: 10px 36px 10px 14px;
    border-radius: 10px;
    font-size: 13px;
  }

  .notice.ok {
    background: color-mix(in srgb, #34c759 16%, var(--surface));
  }

  .notice.error {
    background: color-mix(in srgb, var(--danger) 14%, var(--surface));
  }

  .notice p {
    margin: 0;
  }

  .notice ul {
    margin: 4px 0 0;
    padding-left: 18px;
    list-style: disc;
  }

  .dismiss {
    position: absolute;
    top: 4px;
    right: 6px;
    padding: 2px 8px;
    border: none;
    background: none;
    color: var(--text-secondary);
    font-size: 18px;
    cursor: pointer;
  }

  .selection-bar {
    position: sticky;
    bottom: 16px;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 12px 10px 18px;
    border-radius: var(--radius);
    background: var(--surface);
    box-shadow: 0 6px 24px rgba(0, 0, 0, 0.18);
    font-size: 13px;
    font-variant-numeric: tabular-nums;
  }

  .selection-bar button {
    padding: 5px 14px;
    border: none;
    border-radius: 999px;
    background: color-mix(in srgb, var(--accent) 14%, transparent);
    color: var(--accent);
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
  }

  .selection-bar button.primary {
    background: var(--accent);
    color: #fff;
  }

  .selection-bar button.link {
    margin-left: auto;
    background: none;
  }

  button:disabled {
    opacity: 0.45;
    cursor: not-allowed;
  }
</style>
