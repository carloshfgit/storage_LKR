<!-- Detalhe de uma categoria: total, cards especiais e os maiores arquivos do último scan. -->
<script lang="ts">
  import { homeDir } from "@tauri-apps/api/path";
  import { untrack } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import {
    categoryFiles,
    dockerPrune,
    emptyTrash,
    moveToTrash,
    reveal,
    type Cleanup,
    type DockerItem,
    type DockerMode,
    type DockerTarget,
    type DockerUsage,
    type FileItem,
    type ScanResult,
  } from "./api";
  import type { Segment } from "./categories";
  import CommandLine from "./CommandLine.svelte";
  import ConfirmDialog, { type ConfirmRequest } from "./ConfirmDialog.svelte";
  import { formatBytes, formatCount, formatPercent } from "./format";
  import FileRow from "./FileRow.svelte";
  import { purgeNotice, type Notice } from "./notice";

  let {
    segment,
    result,
    dockerPending,
    total,
    version,
    onback,
    onupdate,
    onopentrash,
    onopendocker,
  }: {
    segment: Segment;
    result: ScanResult;
    dockerPending: boolean;
    total: number;
    /** Muda a cada scan concluído: a lista é recarregada do resultado novo. */
    version: number;
    onback: () => void;
    /** Recebe o resultado atualizado pelo Rust depois de cada ação. */
    onupdate: (result: ScanResult) => void;
    /** Abre o detalhe da Lixeira. */
    onopentrash: () => void;
    /** Abre o detalhe item a item de um daemon Docker. */
    onopendocker: (mode: DockerMode) => void;
  } = $props();

  let files = $state.raw<FileItem[] | null>(null);
  let loadError = $state("");
  let home = $state("");
  /** Incrementado depois de uma ação para recarregar a lista. */
  let reload = $state(0);
  const selection = new SvelteSet<string>();
  /** Ação em andamento; enquanto houver uma, as outras ficam travadas. */
  type PruneKey = `${DockerMode}:${DockerTarget}`;
  let busy = $state<"trash" | "empty" | PruneKey | null>(null);
  let notice = $state<Notice | null>(null);
  let confirm = $state<(ConfirmRequest & { run: () => void }) | null>(null);
  let sortBy = $state<"size" | "date">("size");

  const sorted = $derived(
    !files ? [] : sortBy === "size" ? files : [...files].sort((a, b) => b.modified - a.modified),
  );
  const stats = $derived(result.categories[segment.key]);

  // Só a chave: o `segment` é recriado a cada evento (o `docker`, por exemplo) e não
  // deve recarregar a lista.
  const key = $derived(segment.key);

  $effect(() => {
    const requested = key;
    reload;
    version;
    untrack(() => selection.clear());
    files = null;
    loadError = "";
    categoryFiles(requested).then(
      (list) => requested === key && (files = list),
      (err) => (loadError = String(err)),
    );
  });

  homeDir().then(
    (dir) => (home = dir.replace(/\/$/, "")),
    () => {},
  );

  const selectedBytes = $derived(
    (files ?? []).reduce((sum, f) => sum + (selection.has(f.path) ? f.bytes : 0), 0),
  );

  const fileName = (path: string) => path.slice(path.lastIndexOf("/") + 1);
  const plural = (n: number, one: string, many: string) => `${formatCount(n)} ${n === 1 ? one : many}`;

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

  async function moveSelected() {
    busy = "trash";
    notice = null;
    try {
      const report = await moveToTrash([...selection]);
      onupdate(report.result);
      const parts: string[] = [];
      if (report.moved > 0) {
        parts.push(
          `${plural(report.moved, "arquivo movido", "arquivos movidos")} para a Lixeira (${formatBytes(report.moved_bytes)}).` +
            " Dá para restaurar pela Lixeira do Nautilus; o espaço só é liberado ao esvaziá-la.",
        );
      }
      if (report.rejected.length > 0) {
        parts.push(`${plural(report.rejected.length, "arquivo recusado", "arquivos recusados")}:`);
      }
      notice = {
        kind: report.rejected.length > 0 ? "error" : "ok",
        text: parts.join(" "),
        details: report.rejected.map((r) => ({ text: `${fileName(r.path)}: ${r.reason}` })),
      };
      reload++;
    } catch (err) {
      notice = { kind: "error", text: `Não foi possível mover para a Lixeira: ${err}` };
    } finally {
      busy = null;
    }
  }

  async function emptyAll() {
    busy = "empty";
    notice = null;
    try {
      const report = await emptyTrash();
      onupdate(report.result);
      notice = purgeNotice(report, true);
      reload++;
    } catch (err) {
      notice = { kind: "error", text: `A limpeza falhou: ${err}` };
    } finally {
      busy = null;
    }
  }

  async function runCleanup(kind: PruneKey, action: () => Promise<Cleanup>, done: string) {
    busy = kind;
    notice = null;
    try {
      const cleanup = await action();
      onupdate(cleanup.result);
      notice = { kind: "ok", text: `${done}: ${formatBytes(cleanup.freed_bytes)} liberados no disco.` };
      reload++;
    } catch (err) {
      notice = { kind: "error", text: `A limpeza falhou: ${err}` };
    } finally {
      busy = null;
    }
  }

  function askEmptyTrash() {
    confirm = {
      title: `Apagar permanentemente ${formatBytes(result.trash_bytes)}?`,
      message: "Tudo o que está na Lixeira será apagado para sempre. Não dá para desfazer.",
      confirmLabel: "Esvaziar Lixeira",
      run: emptyAll,
    };
  }

  function askPrune(mode: DockerMode, d: DockerUsage, target: DockerTarget) {
    const key: PruneKey = `${mode}:${target}`;
    const where = mode === "rootless" ? " do Docker rootless" : "";
    confirm =
      target === "build_cache"
        ? {
            title: `Limpar ${formatBytes(d.build_cache.reclaimable)} de build cache${where}?`,
            message: "O Docker recria o cache quando precisar; os próximos builds podem demorar mais.",
            confirmLabel: "Limpar build cache",
            run: () => runCleanup(key, () => dockerPrune(mode, target), "Build cache limpo"),
          }
        : {
            title: `Remover ${formatBytes(d.images.reclaimable)} de imagens não usadas${where}?`,
            message:
              "Imagens que nenhum container usa serão apagadas e precisarão ser baixadas de novo na próxima vez.",
            confirmLabel: "Remover imagens",
            run: () => runCleanup(key, () => dockerPrune(mode, target), "Imagens removidas"),
          };
  }

  function onconfirm() {
    const run = confirm?.run;
    confirm = null;
    run?.();
  }

  /** Um card por daemon presente: o do sistema e o rootless (`~/.local/share/docker`). */
  const daemons = $derived(
    (
      [
        ["rootful", "Docker"],
        ["rootless", "Docker rootless"],
      ] as const
    ).flatMap(([mode, title]) => {
      const usage = result.docker[mode];
      if (!usage) return [];
      const rows = dockerRows(usage);
      return [
        {
          mode,
          title,
          usage,
          rows,
          size: rows.reduce((sum, r) => sum + r.item.size, 0),
          reclaimable: rows.reduce((sum, r) => sum + r.item.reclaimable, 0),
        },
      ];
    }),
  );

  function dockerRows(d: DockerUsage) {
    const rows: { label: string; item: DockerItem; noun: [string, string]; hint?: string }[] = [
      { label: "Imagens", item: d.images, noun: ["imagem", "imagens"] },
      { label: "Containers", item: d.containers, noun: ["container", "containers"] },
      {
        label: "Volumes",
        item: d.volumes,
        noun: ["volume", "volumes"],
        hint: "Guardam dados (bancos, uploads) e nunca são removidos pelo app. Veja quais são em \"Ver detalhes\".",
      },
      { label: "Build cache", item: d.build_cache, noun: ["entrada", "entradas"] },
    ];
    return rows;
  }
</script>

<!-- Com o diálogo aberto, o Esc é dele: fecha o diálogo, não o detalhe. -->
<svelte:window onkeydown={(e) => e.key === "Escape" && !confirm && onback()} />

<button class="back" onclick={onback}>
  <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M10 3 5 8l5 5" /></svg>
  Armazenamento
</button>

<header>
  <h1><span class="dot" style:background={segment.color}></span>{segment.label}</h1>
  <p>
    <strong>{formatBytes(segment.bytes)}</strong>
    · {formatPercent(total > 0 ? segment.bytes / total : 0)} do disco
    {#if stats}· {formatCount(stats.files)} arquivos{/if}
  </p>
</header>

{#if notice}
  <div class="notice {notice.kind}" role="status">
    <p>{notice.text}</p>
    {#if notice.details?.length}
      <ul>
        {#each notice.details as detail}
          <li>
            {detail.text}
            {#each detail.commands ?? [] as command (command)}<CommandLine {command} />{/each}
          </li>
        {/each}
      </ul>
    {/if}
    <button class="dismiss" onclick={() => (notice = null)} aria-label="Fechar aviso">×</button>
  </div>
{/if}

{#if segment.key === "apps_system"}
  {#if dockerPending}
    <section class="card info">
      <h2>Docker</h2>
      <p class="muted">Consultando o Docker…</p>
    </section>
  {:else}
    {#each daemons as daemon (daemon.mode)}
      <section class="card info">
        <h2>
          {daemon.title}
          <span>{formatBytes(daemon.size)} · {formatBytes(daemon.reclaimable)} liberáveis</span>
        </h2>
        {#if daemon.mode === "rootless"}
          <p class="muted">Dados em ~/.local/share/docker, de um daemon do próprio usuário.</p>
        {/if}
        <ul class="rows">
          {#each daemon.rows as row (row.label)}
            <li>
              <span class="label">
                {row.label}
                <small>
                  {formatCount(row.item.count)} {row.noun[row.item.count === 1 ? 0 : 1]}
                  · {formatBytes(row.item.reclaimable)} liberáveis
                </small>
                {#if row.hint}<small>{row.hint}</small>{/if}
              </span>
              <span class="value">{formatBytes(row.item.size)}</span>
            </li>
          {/each}
        </ul>
        <div class="actions">
          <button onclick={() => onopendocker(daemon.mode)} disabled={busy !== null}>Ver detalhes</button>
          <button
            onclick={() => askPrune(daemon.mode, daemon.usage, "build_cache")}
            disabled={busy !== null || daemon.usage.build_cache.reclaimable === 0}
          >
            {busy === `${daemon.mode}:build_cache` ? "Limpando…" : "Limpar build cache"}
          </button>
          <button
            onclick={() => askPrune(daemon.mode, daemon.usage, "unused_images")}
            disabled={busy !== null || daemon.usage.images.reclaimable === 0}
          >
            {busy === `${daemon.mode}:unused_images` ? "Removendo…" : "Remover imagens não usadas"}
          </button>
        </div>
      </section>
    {/each}
  {/if}

  {#if result.unaccounted_bytes > 0}
    <section class="card info">
      <h2>Reservado / sem acesso <span>{formatBytes(result.unaccounted_bytes)}</span></h2>
      <p class="muted">
        Blocos reservados pelo sistema de arquivos, metadados e pastas que só o administrador lê
        ({formatCount(result.denied_dirs)} pastas sem acesso). Não pode ser limpo por aqui.
      </p>
    </section>
  {/if}
{:else if segment.key === "other" && result.trash_bytes > 0}
  <section class="card info">
    <h2>Lixeira <span>{formatBytes(result.trash_bytes)}</span></h2>
    <p class="muted">Arquivos na Lixeira continuam ocupando espaço até ela ser esvaziada.</p>
    <div class="actions">
      <button onclick={onopentrash} disabled={busy !== null}>Ver itens</button>
      <button class="destructive" onclick={askEmptyTrash} disabled={busy !== null}>
        {busy === "empty" ? "Esvaziando…" : "Esvaziar Lixeira"}
      </button>
    </div>
  </section>
{/if}

<section class="card">
  <div class="list-header">
    <h2>{segment.key === "apps_system" ? "Maiores arquivos escaneados" : "Maiores arquivos"}</h2>
    <div class="segmented" role="group" aria-label="Ordenar por">
      <button class:active={sortBy === "size"} onclick={() => (sortBy = "size")}>Tamanho</button>
      <button class:active={sortBy === "date"} onclick={() => (sortBy = "date")}>Data</button>
    </div>
  </div>


  {#if loadError}
    <p class="error">Erro ao carregar os arquivos: {loadError}</p>
  {:else if !files}
    <p class="muted">Carregando…</p>
  {:else if files.length === 0}
    <p class="muted">Nenhum arquivo nesta categoria.</p>
  {:else}
    {#if selection.size === 0 && files.some((f) => f.trashable)}
      <p class="hint">Dica: clique no quadrado ao lado de um item para selecioná-lo.</p>
    {/if}
    <ul class="files">
      {#each sorted as file (file.path)}
        <FileRow
          {file}
          {home}
          selected={selection.has(file.path)}
          disabled={busy !== null}
          ontoggle={() => toggle(file.path)}
          {onreveal}
        />
      {/each}
    </ul>
    <p class="footnote">
      Os {formatCount(files.length)} maiores arquivos{#if sortBy === "date"}, do mais recente ao mais antigo{/if}.
    </p>
  {/if}
</section>

{#if selection.size > 0}
  <div class="selection-bar">
    <span>
      {plural(selection.size, "selecionado", "selecionados")} · <strong>{formatBytes(selectedBytes)}</strong>
    </span>
    <button class="link" onclick={() => selection.clear()} disabled={busy !== null}>Limpar seleção</button>
    <button class="primary" onclick={moveSelected} disabled={busy !== null}>
      {busy === "trash" ? "Movendo…" : "Mover para a Lixeira"}
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

  .back svg {
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
    display: flex;
    align-items: center;
    gap: 10px;
    margin: 0;
    font-size: 28px;
    font-weight: 700;
  }

  h1 .dot {
    width: 14px;
    height: 14px;
    border-radius: 50%;
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

  h2 {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 12px;
    margin: 0;
    font-size: 15px;
    font-weight: 600;
  }

  h2 span {
    color: var(--text-secondary);
    font-weight: 500;
    font-variant-numeric: tabular-nums;
  }

  .info p {
    margin: 6px 0 0;
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

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }

  .rows {
    margin-top: 6px;
  }

  .rows li {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
    padding: 7px 0;
    border-top: 1px solid var(--separator);
    font-variant-numeric: tabular-nums;
  }

  .rows li:first-child {
    border-top: none;
  }

  .label {
    display: flex;
    flex-direction: column;
  }

  .label small {
    color: var(--text-secondary);
    font-size: 12px;
  }

  .value {
    font-weight: 500;
  }

  .list-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 6px;
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

  .actions {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    margin-top: 10px;
  }

  .actions button,
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

  .actions button.destructive {
    background: color-mix(in srgb, var(--danger) 14%, transparent);
    color: var(--danger);
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
</style>
