<!-- Tela principal: disco, barra e legenda. Orquestra o scan e os seus estados. -->
<script lang="ts">
  import "$lib/theme.css";
  import { onMount } from "svelte";
  import {
    CATEGORIES,
    cancelScan,
    diskInfo,
    startScan,
    type Category,
    type DiskInfo,
    type ScanEvent,
    type ScanResult,
  } from "$lib/api";
  import { CATEGORY_COLORS, CATEGORY_LABELS, type Segment } from "$lib/categories";
  import { formatBytes, formatCount, formatSeconds } from "$lib/format";
  import CategoryDetail from "$lib/CategoryDetail.svelte";
  import Legend from "$lib/Legend.svelte";
  import StorageBar from "$lib/StorageBar.svelte";

  type Phase = "idle" | "scanning" | "done" | "cancelled" | "error";

  let phase = $state<Phase>("idle");
  let errorMessage = $state("");
  let disk = $state.raw<DiskInfo | null>(null);
  let live = $state.raw<Partial<Record<Category, number>>>({});
  let liveFiles = $state(0);
  /** Último scan concluído. Continua na tela se o seguinte for cancelado. */
  let result = $state.raw<ScanResult | null>(null);
  /** O `docker` chega uns 5 s depois do `done`. */
  let dockerPending = $state(false);
  /** Ignora eventos atrasados de um Channel de scan anterior. */
  let scanId = 0;
  /** Categoria aberta no detalhe. */
  let selected = $state<Category | null>(null);

  const showLive = $derived(phase === "scanning" || !result);
  // O detalhe lê o último resultado completo; durante o scan ele ainda não existe.
  const onselect = $derived(showLive ? undefined : open);

  function open(key: Category) {
    selected = key;
    window.scrollTo(0, 0);
  }

  const segments = $derived.by((): Segment[] =>
    CATEGORIES.map((key) => {
      const segment: Segment = { key, label: CATEGORY_LABELS[key], color: CATEGORY_COLORS[key], bytes: 0 };
      if (showLive) {
        segment.bytes = live[key] ?? 0;
        return segment;
      }
      segment.bytes = result!.categories[key]?.bytes ?? 0;
      if (key === "apps_system") {
        // O que o walk não leu fecha a barra dentro de Apps/Sistema (DECISIONS D05).
        segment.bytes += result!.docker_bytes + result!.unaccounted_bytes;
        segment.note = appsNote(result!);
      }
      return segment;
    }),
  );

  const detail = $derived(selected && !showLive ? segments.find((s) => s.key === selected) : undefined);

  const free = $derived(disk?.available_bytes ?? 0);
  const used = $derived(disk ? disk.total_bytes - disk.available_bytes : 0);
  // O restante é `max(0, usado − escaneado)`; se o escaneado passar do usado, a barra
  // é normalizada pela soma para nunca estourar 100%.
  const total = $derived(Math.max(disk?.total_bytes ?? 0, free + segments.reduce((sum, s) => sum + s.bytes, 0)));

  function appsNote(r: ScanResult): string | undefined {
    const unaccounted = `${formatBytes(r.unaccounted_bytes)} reservado/sem acesso`;
    if (dockerPending) return `Inclui ${unaccounted} · consultando Docker…`;
    if (r.docker_bytes > 0) return `Inclui ${formatBytes(r.docker_bytes)} do Docker e ${unaccounted}`;
    return r.unaccounted_bytes > 0 ? `Inclui ${unaccounted}` : undefined;
  }

  function onEvent(e: ScanEvent) {
    switch (e.kind) {
      case "progress":
        live = e.bytes;
        liveFiles = e.files;
        break;
      case "done":
        result = e.result;
        disk = e.result.disk;
        dockerPending = true;
        phase = "done";
        break;
      case "docker":
        if (result) {
          result = { ...result, docker: e.usage, docker_bytes: e.docker_bytes, unaccounted_bytes: e.unaccounted_bytes };
        }
        dockerPending = false;
        break;
      case "cancelled":
        // O `disk` foi relido no início deste scan; o anterior volta com o disco dele.
        if (result) disk = result.disk;
        phase = "cancelled";
        break;
      case "error":
        fail(e.message);
        break;
    }
  }

  function fail(err: unknown) {
    errorMessage = String(err);
    dockerPending = false;
    phase = "error";
  }

  async function scan() {
    const id = ++scanId;
    live = {};
    liveFiles = 0;
    dockerPending = false;
    phase = "scanning";
    try {
      // "Livre" atualizado antes do scan: pode ter mudado desde a última leitura.
      disk = await diskInfo();
      await startScan((e) => id === scanId && onEvent(e));
    } catch (err) {
      if (id === scanId) fail(err);
    }
  }

  async function cancel() {
    try {
      await cancelScan();
    } catch (err) {
      fail(err);
    }
  }

  onMount(scan);

  const diskName = $derived(!disk ? "" : disk.mount === "/" ? "Disco do sistema" : disk.mount);
</script>

<main>
  {#if detail && result}
    <CategoryDetail segment={detail} {result} {dockerPending} {total} onback={() => (selected = null)} />
  {/if}

  <!-- Escondida, não desmontada: a barra volta do detalhe sem refazer a animação. -->
  <div hidden={!!detail}>
    <header>
      <h1>Armazenamento</h1>
      {#if disk}
        <p class="disk">{diskName}{#if disk.device}<span> · {disk.device}</span>{/if}</p>
      {/if}
    </header>

    <section class="card">
      <p class="usage">
        {#if disk}
          <strong>{formatBytes(used)}</strong> de {formatBytes(disk.total_bytes)} usados
        {:else}
          Lendo o disco…
        {/if}
      </p>

      <StorageBar {segments} {free} {total} scanning={phase === "scanning"} {onselect} />

      <div class="status" aria-live="polite">
        {#if phase === "scanning"}
          <span class="spinner" aria-hidden="true"></span>
          <span>Escaneando… {formatCount(liveFiles)} arquivos</span>
          <button onclick={cancel}>Cancelar</button>
        {:else if phase === "done" && result}
          <span>{formatCount(result.files)} arquivos analisados em {formatSeconds(result.elapsed_ms)}</span>
          <button onclick={scan}>Reescanear</button>
        {:else if phase === "cancelled"}
          <span>Escaneamento cancelado{#if result} · mostrando o anterior{/if}</span>
          <button onclick={scan}>Reescanear</button>
        {:else if phase === "error"}
          <span class="error">Erro: {errorMessage}</span>
          <button onclick={scan}>Tentar de novo</button>
        {/if}
      </div>

      <Legend {segments} {free} {total} {onselect} />
    </section>
  </div>
</main>

<style>
  main {
    max-width: 720px;
    margin: 0 auto;
    padding: 28px 24px 32px;
  }

  header {
    margin-bottom: 16px;
  }

  h1 {
    margin: 0;
    font-size: 28px;
    font-weight: 700;
    letter-spacing: -0.01em;
  }

  .disk {
    margin: 2px 0 0;
    color: var(--text-secondary);
  }

  .disk span {
    font-size: 13px;
  }

  .card {
    background: var(--surface);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
    padding: 18px 20px 8px;
  }

  .usage {
    margin: 0 0 12px;
    color: var(--text-secondary);
    font-variant-numeric: tabular-nums;
  }

  .usage strong {
    color: var(--text);
    font-size: 20px;
    font-weight: 600;
  }

  .status {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 32px;
    margin: 10px 0 6px;
    color: var(--text-secondary);
    font-size: 13px;
    font-variant-numeric: tabular-nums;
  }

  .status button {
    margin-left: auto;
    padding: 5px 14px;
    border: none;
    border-radius: 999px;
    background: color-mix(in srgb, var(--accent) 14%, transparent);
    color: var(--accent);
    font-weight: 600;
    font-size: 13px;
    cursor: pointer;
    transition: background var(--duration) var(--ease);
  }

  .status button:hover {
    background: color-mix(in srgb, var(--accent) 22%, transparent);
  }

  .error {
    color: var(--danger);
  }

  .spinner {
    width: 12px;
    height: 12px;
    border: 2px solid var(--track);
    border-top-color: var(--accent);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
</style>
