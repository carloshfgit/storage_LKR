<!-- Página provisória da Etapa 3: dispara o scan e mostra os eventos crus.
     A interface de verdade (barra, legenda) entra na Etapa 4. -->
<script lang="ts">
  import { onMount } from "svelte";
  import {
    CATEGORIES,
    cancelScan,
    categoryFiles,
    diskInfo,
    startScan,
    type Category,
    type DiskInfo,
    type FileEntry,
    type ScanEvent,
  } from "$lib/api";

  let disk = $state<DiskInfo | null>(null);
  let bytes = $state<Partial<Record<Category, number>>>({});
  let files = $state(0);
  let status = $state("parado");
  let log = $state<string[]>([]);
  let top = $state<FileEntry[]>([]);

  const gb = (b: number) => (b / 1e9).toFixed(2) + " GB";

  function onEvent(e: ScanEvent) {
    console.log("scan event", e);
    if (e.kind === "progress") {
      bytes = e.bytes;
      files = e.files;
      return;
    }
    if (e.kind === "done") {
      status = `concluído em ${(e.result.elapsed_ms / 1000).toFixed(2)} s — aguardando Docker`;
    } else if (e.kind === "docker") {
      status = "concluído";
    } else if (e.kind === "cancelled") {
      status = "cancelado";
    } else {
      status = "erro: " + e.message;
    }
    log = [...log, `${e.kind}: ${JSON.stringify(e).slice(0, 300)}`];
  }

  async function scan() {
    log = [];
    top = [];
    status = "escaneando";
    try {
      await startScan(onEvent);
    } catch (err) {
      status = "erro: " + err;
    }
  }

  async function showTop(c: Category) {
    try {
      top = (await categoryFiles(c)).slice(0, 10);
    } catch (err) {
      status = "erro: " + err;
    }
  }

  onMount(async () => {
    disk = await diskInfo();
  });
</script>

<main>
  <h1>Ponte Tauri — teste</h1>
  {#if disk}
    <p>{disk.device} em {disk.mount}: {gb(disk.total_bytes - disk.available_bytes)} usados de {gb(disk.total_bytes)}</p>
  {/if}

  <button onclick={scan}>Escanear</button>
  <button onclick={cancelScan}>Cancelar</button>
  <span>{status} · {files} arquivos</span>

  <table>
    <tbody>
      {#each CATEGORIES as c}
        <tr>
          <td>{c}</td>
          <td>{gb(bytes[c] ?? 0)}</td>
          <td><button onclick={() => showTop(c)}>maiores</button></td>
        </tr>
      {/each}
    </tbody>
  </table>

  {#each top as f}
    <div>{gb(f.bytes)} — {f.path}</div>
  {/each}

  <pre>{log.join("\n")}</pre>
</main>

<style>
  main { font-family: system-ui, sans-serif; padding: 16px; }
  td { padding: 2px 12px 2px 0; }
  pre { white-space: pre-wrap; font-size: 12px; }
</style>
