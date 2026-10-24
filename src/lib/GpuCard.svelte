<!-- Card GPU da aba Recursos (PLAN-V2, Etapa 18): uma placa por vez, com abas pequenas quando há mais
     de uma (integrada + dedicada). Mostra o que o driver expõe e, no lugar do que falta, o motivo
     (DECISIONS D49). Placa em repouso não é lida, e o card diz isso em vez de mostrar zeros. -->
<script lang="ts">
  import type { GpuSample, Sample } from "./api";
  import { formatBytes, formatMHz, formatPercent } from "./format";
  import MetricCard from "./MetricCard.svelte";
  import { GPU_VENDOR_LABELS, gpuKind, gpuName } from "./resources";
  import Sparkline from "./Sparkline.svelte";
  import TempValue from "./TempValue.svelte";

  let { samples }: { samples: Sample[] } = $props();

  const gpus = $derived(samples.at(-1)?.gpus ?? []);
  /** Placa das abas; começa na primeira acordada. */
  let chosen = $state<string | null>(null);
  $effect(() => {
    if (chosen === null && gpus.length > 0) chosen = (gpus.find((g) => g.state === "active") ?? gpus[0]).card;
  });
  const gpu = $derived(gpus.find((g) => g.card === chosen) ?? gpus[0]);

  const pct = (p: number) => formatPercent(p / 100);
  const watts = (w: number) => `${w.toLocaleString("pt-BR", { maximumFractionDigits: 1, minimumFractionDigits: 1 })} W`;

  /** O motivo de um número faltar; sem motivo conhecido (ainda sem leitura), "—". */
  function missing(g: GpuSample, field: "busy" | "power" | "other"): string {
    if (g.note === null) return "—";
    if (g.note === "needs_root" && field !== "other") return "exige root";
    return "não exposto pelo driver";
  }

  /** Com uso, o gráfico é o uso; sem ele (Intel), a frequência em % da máxima. */
  const byFreq = $derived(gpu.busy_pct === null && gpu.freq_mhz !== null && gpu.freq_max_mhz !== null);
  const values = $derived(
    samples.map((s) => {
      const g = s.gpus.find((x) => x.card === gpu.card);
      if (!g || g.state !== "active") return null;
      if (!byFreq) return g.busy_pct;
      return g.freq_mhz !== null && g.freq_max_mhz ? (g.freq_mhz / g.freq_max_mhz) * 100 : null;
    }),
  );

  const value = $derived(
    gpu.state === "suspended"
      ? "Em repouso"
      : gpu.busy_pct !== null
        ? pct(gpu.busy_pct)
        : gpu.freq_mhz !== null
          ? formatMHz(gpu.freq_mhz)
          : "—",
  );
  const subtitle = $derived([gpuName(gpu), gpuKind(gpu)].filter(Boolean).join(" · "));

  function tabLabel(g: GpuSample): string {
    const vendor = GPU_VENDOR_LABELS[g.vendor] || g.card;
    const kind = gpuKind(g);
    return kind ? `${vendor} · ${kind}` : vendor;
  }
</script>

<MetricCard title="GPU" {value} {subtitle}>
  {#snippet actions()}
    {#if gpus.length > 1}
      <div class="tabs" role="group" aria-label="Placa de vídeo">
        {#each gpus as g (g.card)}
          <button aria-pressed={g.card === gpu.card} title={gpuName(g)} onclick={() => (chosen = g.card)}>
            {tabLabel(g)}
          </button>
        {/each}
      </div>
    {/if}
  {/snippet}

  {#if gpu.state === "suspended"}
    <p class="meta">
      O sistema desligou a placa para economizar energia. Ela não é consultada, para não acordar; os números
      voltam quando algum programa a usar.
    </p>
  {:else}
    {#if gpu.busy_pct !== null || byFreq}
      <Sparkline
        max={100}
        label={byFreq ? "Frequência da GPU, em % da máxima, no último minuto" : "Uso da GPU no último minuto"}
        series={[{ label: "GPU", color: "var(--res-gpu)", values }]}
      />
    {/if}
    <dl class="rows">
      <div>
        <dt>Uso</dt>
        <dd>
          {#if gpu.busy_pct !== null}{pct(gpu.busy_pct)}{:else}<span class="why">{missing(gpu, "busy")}</span>{/if}
        </dd>
      </div>
      <div>
        <dt>Memória de vídeo</dt>
        <dd>
          {#if gpu.vram_used !== null && gpu.vram_total !== null}
            {formatBytes(gpu.vram_used)} de {formatBytes(gpu.vram_total)}
          {:else if gpu.integrated && gpu.vram_total === null}
            <span class="why">usa a RAM</span>
          {:else}
            <span class="why">{missing(gpu, "other")}</span>
          {/if}
        </dd>
      </div>
      {#if gpu.gtt_used !== null && gpu.gtt_total !== null}
        <div>
          <dt title="Memória do sistema que a GPU integrada usa (GTT)">Memória compartilhada</dt>
          <dd>{formatBytes(gpu.gtt_used)} de {formatBytes(gpu.gtt_total)}</dd>
        </div>
      {/if}
      <div>
        <dt>Frequência</dt>
        <dd>
          {#if gpu.freq_mhz !== null}
            {formatMHz(gpu.freq_mhz)}{#if gpu.freq_max_mhz}<small> de {formatMHz(gpu.freq_max_mhz)}</small>{/if}
          {:else}
            <span class="why">{missing(gpu, "other")}</span>
          {/if}
        </dd>
      </div>
      <div>
        <dt>Temperatura</dt>
        <dd>
          {#if gpu.temp}<TempValue temp={gpu.temp} />{:else}<span class="why">{missing(gpu, "other")}</span>{/if}
        </dd>
      </div>
      <div>
        <dt>Consumo</dt>
        <dd>
          {#if gpu.power_w !== null}{watts(gpu.power_w)}{:else}<span class="why">{missing(gpu, "power")}</span>{/if}
        </dd>
      </div>
    </dl>
    {#if gpu.note === "needs_root"}
      <p class="meta hint">O uso e o consumo da GPU Intel só são medidos com root (como no intel_gpu_top).</p>
    {:else if gpu.note === "no_nvidia_smi"}
      <p class="meta hint">
        Os números da NVIDIA vêm do nvidia-smi, que não foi encontrado. Ele vem no pacote nvidia-utils do driver.
      </p>
    {/if}
  {/if}
</MetricCard>

<style>
  .tabs {
    display: flex;
    gap: 2px;
    padding: 2px;
    border-radius: 7px;
    background: var(--track);
  }

  .tabs button {
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

  .tabs button[aria-pressed="true"] {
    background: var(--surface);
    color: var(--text);
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.12);
  }

  .tabs button:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }

  @media (prefers-color-scheme: dark) {
    .tabs button[aria-pressed="true"] {
      background: #636366;
    }
  }

  .meta {
    margin: 0;
    color: var(--text-secondary);
    font-size: 12px;
  }

  .hint {
    margin-top: 4px;
  }

  .rows {
    margin: 4px 0 0;
  }

  .rows div {
    display: flex;
    justify-content: space-between;
    gap: 12px;
    padding: 5px 0;
    border-top: 1px solid var(--separator);
    font-size: 13px;
  }

  dt {
    color: var(--text-secondary);
  }

  dd {
    margin: 0;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }

  small,
  .why {
    color: var(--text-secondary);
  }

  small {
    font-size: 11px;
  }

  .why {
    font-size: 12px;
  }
</style>
