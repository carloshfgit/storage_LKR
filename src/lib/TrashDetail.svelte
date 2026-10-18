<!-- Detalhe da Lixeira: os itens de agora (nome, local original, quando foi apagado e
     tamanho), com restaurar, apagar os selecionados e esvaziar tudo. -->
<script lang="ts">
  import { homeDir } from "@tauri-apps/api/path";
  import { untrack } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import {
    emptyTrash,
    purgeFromTrash,
    restoreFromTrash,
    reveal,
    trashItems,
    type ScanResult,
    type TrashItem,
  } from "./api";
  import ConfirmDialog, { type ConfirmRequest } from "./ConfirmDialog.svelte";
  import { formatBytes, formatCount, formatRelativeDate } from "./format";

  let {
    result,
    version,
    onback,
    onupdate,
  }: {
    result: ScanResult;
    /** Muda a cada scan concluído: a lista é recarregada. */
    version: number;
    onback: () => void;
    /** Recebe o resultado atualizado pelo Rust depois de cada ação. */
    onupdate: (result: ScanResult) => void;
  } = $props();

  let items = $state.raw<TrashItem[] | null>(null);
  let loadError = $state("");
  let home = $state("");
  /** Incrementado depois de uma ação para recarregar a lista. */
  let reload = $state(0);
  const selection = new SvelteSet<string>();
  let busy = $state<"restore" | "purge" | "empty" | null>(null);
  let notice = $state<{ kind: "ok" | "error"; text: string; details?: string[] } | null>(null);
  let confirm = $state<(ConfirmRequest & { run: () => void }) | null>(null);
  let sortBy = $state<"size" | "date">("size");

  $effect(() => {
    reload;
    version;
    untrack(() => selection.clear());
    trashItems().then(
      (list) => {
        items = list;
        loadError = "";
      },
      (err) => (loadError = String(err)),
    );
  });

  homeDir().then(
    (dir) => (home = dir.replace(/\/$/, "")),
    () => {},
  );

  const sorted = $derived(
    !items ? [] : sortBy === "size" ? items : [...items].sort((a, b) => b.deleted_at - a.deleted_at),
  );
  const totalBytes = $derived((items ?? []).reduce((sum, i) => sum + i.bytes, 0));
  const uncounted = $derived((items ?? []).filter((i) => !i.counted).length);
  const selectedBytes = $derived((items ?? []).reduce((sum, i) => sum + (selection.has(i.id) ? i.bytes : 0), 0));

  const plural = (n: number, one: string, many: string) => `${formatCount(n)} ${n === 1 ? one : many}`;
  const fileName = (path: string) => path.slice(path.lastIndexOf("/") + 1);
  function folder(path: string) {
    const dir = path.slice(0, path.lastIndexOf("/")) || "/";
    return home && (dir === home || dir.startsWith(home + "/")) ? "~" + dir.slice(home.length) : dir;
  }
  /** Os recusados voltam com o caminho original ou, se nem estavam na Lixeira, com o id. */
  const rejectedName = (path: string) => fileName(path).replace(/\.trashinfo$/, "");

  function toggle(id: string) {
    if (!selection.delete(id)) selection.add(id);
  }

  async function onreveal(path: string) {
    try {
      await reveal(path);
    } catch (err) {
      notice = { kind: "error", text: `Não foi possível abrir a pasta: ${err}` };
    }
  }

  async function restoreSelected() {
    busy = "restore";
    notice = null;
    try {
      const report = await restoreFromTrash([...selection]);
      onupdate(report.result);
      const parts: string[] = [];
      if (report.moved > 0) {
        parts.push(
          `${plural(report.moved, "item restaurado", "itens restaurados")} (${formatBytes(report.moved_bytes)}), de volta ao local original.`,
        );
      }
      if (report.rejected.length > 0) {
        parts.push(`${plural(report.rejected.length, "item recusado", "itens recusados")}:`);
      }
      notice = {
        kind: report.rejected.length > 0 ? "error" : "ok",
        text: parts.join(" "),
        details: report.rejected.map((r) => `${rejectedName(r.path)}: ${r.reason}`),
      };
      reload++;
    } catch (err) {
      notice = { kind: "error", text: `Não foi possível restaurar: ${err}` };
    } finally {
      busy = null;
    }
  }

  async function purgeSelected(ids: string[]) {
    busy = "purge";
    notice = null;
    try {
      const report = await purgeFromTrash(ids);
      onupdate(report.result);
      const parts: string[] = [];
      if (report.purged > 0) {
        parts.push(
          `${plural(report.purged, "item apagado", "itens apagados")}: ${formatBytes(report.freed_bytes)} liberados no disco.`,
        );
      }
      if (report.rejected.length > 0) {
        parts.push(`${plural(report.rejected.length, "item recusado", "itens recusados")}:`);
      }
      notice = {
        kind: report.rejected.length > 0 ? "error" : "ok",
        text: parts.join(" "),
        details: report.rejected.map((r) => `${rejectedName(r.path)}: ${r.reason}`),
      };
      reload++;
    } catch (err) {
      notice = { kind: "error", text: `Não foi possível apagar: ${err}` };
    } finally {
      busy = null;
    }
  }

  async function emptyAll() {
    busy = "empty";
    notice = null;
    try {
      const cleanup = await emptyTrash();
      onupdate(cleanup.result);
      notice = { kind: "ok", text: `Lixeira esvaziada: ${formatBytes(cleanup.freed_bytes)} liberados no disco.` };
      reload++;
    } catch (err) {
      notice = { kind: "error", text: `A limpeza falhou: ${err}` };
    } finally {
      busy = null;
    }
  }

  function askPurge() {
    const ids = [...selection];
    confirm = {
      title: `Apagar permanentemente ${plural(ids.length, "item", "itens")} (${formatBytes(selectedBytes)})?`,
      message: "Os itens selecionados serão apagados para sempre. Não dá para desfazer.",
      confirmLabel: "Apagar",
      run: () => purgeSelected(ids),
    };
  }

  function askEmpty() {
    confirm = {
      title: `Apagar permanentemente ${formatBytes(totalBytes)}?`,
      message: "Tudo o que está na Lixeira será apagado para sempre. Não dá para desfazer.",
      confirmLabel: "Esvaziar Lixeira",
      run: emptyAll,
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
  <h1>Lixeira</h1>
  {#if items}
    <p>
      <strong>{formatBytes(totalBytes)}</strong> · {items.length === 0
        ? "vazia"
        : `${plural(items.length, "item", "itens")}; o espaço só é liberado ao apagá-los`}
    </p>
  {/if}
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
  {#if loadError}
    <p class="error">Erro ao ler a Lixeira: {loadError}</p>
  {:else if !items}
    <p class="muted">Carregando…</p>
  {:else if items.length === 0}
    <p class="muted">A Lixeira está vazia.</p>
  {:else}
    <div class="list-header">
      <div class="segmented" role="group" aria-label="Ordenar por">
        <button class:active={sortBy === "size"} onclick={() => (sortBy = "size")}>Tamanho</button>
        <button class:active={sortBy === "date"} onclick={() => (sortBy = "date")}>Data</button>
      </div>
      <button class="destructive" onclick={askEmpty} disabled={busy !== null}>
        {busy === "empty" ? "Esvaziando…" : "Esvaziar Lixeira"}
      </button>
    </div>
    {#if selection.size === 0}
      <p class="hint">Dica: clique no quadrado ao lado de um item para selecioná-lo.</p>
    {/if}
    <ul class="items">
      {#each sorted as item (item.id)}
        {@const date = new Date(item.deleted_at * 1000).toLocaleString("pt-BR")}
        <li>
          <input
            type="checkbox"
            checked={selection.has(item.id)}
            disabled={busy !== null}
            onchange={() => toggle(item.id)}
            aria-label="Selecionar {item.name}"
          />
          <div class="text">
            <span class="name" title={item.name}>{item.name}</span>
            <span class="folder" title={item.original_path}>
              {folder(item.original_path)}{#if item.files > 1} · {plural(item.files, "arquivo", "arquivos")}{/if}
            </span>
          </div>
          <span class="meta">
            <span class="size" title={item.counted ? undefined : "Medido agora; entra na barra no próximo scan"}>
              {formatBytes(item.bytes)}{#if !item.counted}*{/if}
            </span>
            <span class="date" title={date}>apagado {formatRelativeDate(item.deleted_at)}</span>
          </span>
          <button
            class="reveal"
            onclick={() => onreveal(item.stored_path)}
            disabled={item.stored_path.includes("�")}
            title="Abrir na pasta"
            aria-label="Abrir {item.name} na pasta"
          >
            <svg viewBox="0 0 16 16" aria-hidden="true">
              <path d="M1.5 4.5v8a1 1 0 0 0 1 1h11a1 1 0 0 0 1-1v-6a1 1 0 0 0-1-1H8L6.5 3.5h-4a1 1 0 0 0-1 1Z" />
            </svg>
          </button>
        </li>
      {/each}
    </ul>
    {#if uncounted > 0}
      <p class="footnote">
        * {plural(uncounted, "item foi", "itens foram")} para a Lixeira depois da análise: o tamanho foi medido agora
        e entra na barra no próximo scan.
      </p>
    {/if}
  {/if}
</section>

{#if selection.size > 0}
  <div class="selection-bar">
    <span>
      {plural(selection.size, "selecionado", "selecionados")} · <strong>{formatBytes(selectedBytes)}</strong>
    </span>
    <button class="link" onclick={() => selection.clear()} disabled={busy !== null}>Limpar seleção</button>
    <button class="destructive" onclick={askPurge} disabled={busy !== null}>
      {busy === "purge" ? "Apagando…" : "Apagar"}
    </button>
    <button class="primary" onclick={restoreSelected} disabled={busy !== null}>
      {busy === "restore" ? "Restaurando…" : "Restaurar"}
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

  .hint {
    margin: 0 0 6px;
    color: var(--text-secondary);
    font-size: 13px;
  }

  .error {
    color: var(--danger);
    font-size: 13px;
  }

  .list-header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    margin-bottom: 6px;
  }

  .segmented {
    display: flex;
    padding: 2px;
    border-radius: 8px;
    background: var(--track);
  }

  .segmented button {
    padding: 3px 12px;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: var(--text);
    font-size: 13px;
    cursor: pointer;
    transition: background var(--duration) var(--ease);
  }

  .segmented button.active {
    background: var(--surface);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.12);
    font-weight: 600;
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  /* Mesma grade do FileRow: checkbox, texto, tamanho e data, "abrir na pasta". */
  .items li {
    display: grid;
    grid-template-columns: 18px 1fr auto 32px;
    align-items: center;
    gap: 10px;
    padding: 8px 0;
    border-top: 1px solid var(--separator);
  }

  .items li:first-child {
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

  .folder,
  .date {
    color: var(--text-secondary);
    font-size: 12px;
  }

  .meta {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
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

  .list-header > button,
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

  button.destructive {
    background: color-mix(in srgb, var(--danger) 14%, transparent);
    color: var(--danger);
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
