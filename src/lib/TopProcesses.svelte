<!-- Card Processos da aba Recursos (PLAN-V2, Etapa 19): os 5 aplicativos que mais usam CPU, memória ou
     disco, com os processos de cada um somados ("Google Chrome ×22", DECISIONS D50). Só leitura: não há
     "encerrar". A lista chega a cada 2 s; a troca de posição anima, menos com movimento reduzido. -->
<script lang="ts">
  import { flip } from "svelte/animate";
  import type { ProcGroup, ProcTop } from "./api";
  import { formatBytes, formatCount, formatPercent, formatRate } from "./format";
  import MetricCard from "./MetricCard.svelte";
  import MiniTabs from "./MiniTabs.svelte";

  let { top, threads, ramTotal }: { top: ProcTop | null; threads: number; ramTotal: number } = $props();

  type Sort = "cpu" | "mem" | "disk";
  type CpuScale = "core" | "total";
  const SORT_KEY = "storage-looker.procs.sort";
  const SCALE_KEY = "storage-looker.procs.cpu-scale";

  function load<T extends string>(key: string, allowed: T[], fallback: T): T {
    try {
      const v = localStorage.getItem(key) as T | null;
      return v && allowed.includes(v) ? v : fallback;
    } catch {
      return fallback;
    }
  }

  function save(key: string, value: string) {
    try {
      localStorage.setItem(key, value);
    } catch {
      // Sem armazenamento local: só não lembra a escolha.
    }
  }

  let sort = $state<Sort>(load(SORT_KEY, ["cpu", "mem", "disk"], "cpu"));
  let scale = $state<CpuScale>(load(SCALE_KEY, ["core", "total"], "core"));
  const reduced = window.matchMedia("(prefers-reduced-motion: reduce)").matches;

  const SORTS: { id: Sort; label: string }[] = [
    { id: "cpu", label: "CPU" },
    { id: "mem", label: "Memória" },
    { id: "disk", label: "Disco" },
  ];

  const list = $derived(top ? top[sort] : []);
  const cores = $derived(Math.max(threads, 1));
  const diskRate = (g: ProcGroup) => (g.read_bps ?? 0) + (g.write_bps ?? 0);
  const diskPeak = $derived(Math.max(...list.map(diskRate), 1));

  function value(g: ProcGroup): string {
    if (sort === "mem") return formatBytes(g.mem_bytes);
    if (sort === "disk") return formatRate(diskRate(g));
    const pct = g.cpu_pct ?? 0;
    return formatPercent((scale === "core" ? pct : pct / cores) / 100);
  }

  function hint(g: ProcGroup): string | undefined {
    if (sort === "disk") return `Leitura ${formatRate(g.read_bps ?? 0)} · gravação ${formatRate(g.write_bps ?? 0)}`;
    if (sort === "cpu" && g.cpu_pct !== null) {
      return `${formatPercent(g.cpu_pct / 100)} de um núcleo · ${formatPercent(g.cpu_pct / cores / 100)} do processador`;
    }
    return undefined;
  }

  /** Fração da mini-barra: CPU contra um núcleo (ou o processador), memória contra a RAM, disco contra o maior. */
  function fill(g: ProcGroup): number {
    if (sort === "mem") return ramTotal ? g.mem_bytes / ramTotal : 0;
    if (sort === "disk") return diskRate(g) / diskPeak;
    const pct = (g.cpu_pct ?? 0) / 100;
    return Math.min(scale === "core" ? pct : pct / cores, 1);
  }

  const color = $derived(
    sort === "cpu" ? "var(--res-cpu)" : sort === "mem" ? "var(--res-mem)" : "var(--res-disk-write)",
  );

  const empty = $derived(
    !top
      ? "Lendo os processos…"
      : sort === "cpu"
        ? "Medindo o uso de CPU…"
        : sort === "disk"
          ? "Nenhum processo seu está lendo ou gravando agora."
          : "Nenhum processo.",
  );
</script>

<MetricCard
  title="Processos"
  value={top ? formatCount(top.total) : "—"}
  subtitle="em execução · agrupados por aplicativo"
>
  {#snippet actions()}
    <MiniTabs
      label="Ordenar por"
      options={SORTS}
      value={sort}
      onchange={(s) => {
        sort = s;
        save(SORT_KEY, s);
      }}
    />
  {/snippet}

  {#if list.length === 0}
    <p class="meta">{empty}</p>
  {:else}
    <ol class="list" style:--bar={color}>
      {#each list as g (g.key)}
        <li animate:flip={{ duration: reduced ? 0 : 250 }}>
          <span class="name" title={g.name}>
            {g.name}{#if g.count > 1}<small> ×{g.count}</small>{/if}
          </span>
          <span class="value" title={hint(g)}>{value(g)}</span>
          <span class="bar" style:--fill="{fill(g) * 100}%"></span>
        </li>
      {/each}
    </ol>
  {/if}

  {#if sort === "cpu"}
    <p class="meta">
      {scale === "core" ? "Em % de um núcleo, como no top." : `Em % do processador inteiro (${cores} threads).`}
      <button
        class="scale"
        onclick={() => {
          scale = scale === "core" ? "total" : "core";
          save(SCALE_KEY, scale);
        }}
      >
        {scale === "core" ? "Ver do processador" : "Ver de um núcleo"}
      </button>
    </p>
  {:else if sort === "disk"}
    <p class="meta">Só os seus processos: os de outros usuários não informam leitura e gravação.</p>
  {:else}
    <p class="meta">A memória compartilhada entre os processos de um app conta em cada um.</p>
  {/if}
</MetricCard>

<style>
  .list {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: grid;
    grid-template-columns: 1fr auto;
    align-items: baseline;
    column-gap: 10px;
    padding: 6px 0 5px;
    border-top: 1px solid var(--separator);
    font-size: 13px;
  }

  .name {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  small {
    color: var(--text-secondary);
    font-size: 11px;
  }

  .value {
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .bar {
    grid-column: 1 / -1;
    height: 4px;
    margin-top: 4px;
    border-radius: 2px;
    background: linear-gradient(to right, var(--bar) var(--fill), var(--track) var(--fill));
  }

  .meta {
    margin: 0;
    color: var(--text-secondary);
    font-size: 12px;
  }

  .scale {
    padding: 0;
    border: none;
    background: none;
    color: var(--accent);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }

  .scale:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
  }
</style>
