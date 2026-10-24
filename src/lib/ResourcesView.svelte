<!-- Aba Recursos: CPU, memória, disco, rede, GPU e bateria em tempo real, e a ficha técnica (PLAN-V2,
     Etapas 16 a 18).
     Só coleta com a aba ativa, a página visível e a janela não minimizada (DECISIONS D46). Perder o
     foco não pausa: o caso de uso é olhar o monitor enquanto outra janela faz o trabalho pesado.
     O histórico (60 amostras) fica só aqui e recomeça a cada sessão (D47). -->
<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import {
    startMonitor,
    stopMonitor,
    systemInfo,
    type MonitorEvent,
    type Sample,
    type SystemInfo,
  } from "./api";
  import BatteryCard from "./BatteryCard.svelte";
  import CommandLine from "./CommandLine.svelte";
  import { formatBytes, formatMHz, formatPercent, formatRate } from "./format";
  import GpuCard from "./GpuCard.svelte";
  import MetricCard from "./MetricCard.svelte";
  import { diskKind } from "./resources";
  import Sparkline from "./Sparkline.svelte";
  import SystemInfoCard from "./SystemInfoCard.svelte";
  import TempValue from "./TempValue.svelte";

  let { active }: { active: boolean } = $props();

  const CAPACITY = 60;
  /** PSI `some avg10` acima disso: alguma tarefa esperou por memória em mais de 10% do tempo. */
  const PRESSURE_PCT = 10;

  let visible = $state(document.visibilityState === "visible");
  let minimized = $state(false);
  let samples = $state.raw<Sample[]>([]);
  let info = $state.raw<SystemInfo | null>(null);
  let error = $state("");
  /** Incrementa em "Tentar de novo" para reabrir a sessão. */
  let attempt = $state(0);
  /** Ignora eventos de uma sessão que já acabou. */
  let session = 0;

  const running = $derived(active && visible && !minimized);

  function onEvent(id: number, e: MonitorEvent) {
    if (id !== session) return;
    if (e.kind === "sample") {
      const next = samples.length >= CAPACITY ? samples.slice(1) : samples.slice();
      next.push(e.sample);
      samples = next;
    } else if (e.kind === "error") {
      error = e.message;
    }
  }

  $effect(() => {
    if (!running) return;
    void attempt;
    const id = ++session;
    samples = [];
    error = "";
    startMonitor((e) => onEvent(id, e)).catch((err) => {
      if (id === session) error = String(err);
    });
    return () => {
      session++;
      stopMonitor().catch(() => {});
    };
  });

  onMount(() => {
    systemInfo()
      .then((i) => (info = i))
      .catch(() => {});

    const onVisibility = () => (visible = document.visibilityState === "visible");
    document.addEventListener("visibilitychange", onVisibility);

    // O WebKitGTK nem sempre marca a página como oculta ao minimizar: a janela diz.
    const win = getCurrentWindow();
    const check = () =>
      win
        .isMinimized()
        .then((m) => (minimized = m))
        .catch(() => {});
    const unlisten = [win.onResized(check), win.onFocusChanged(check)];
    return () => {
      document.removeEventListener("visibilitychange", onVisibility);
      for (const u of unlisten) u.then((f) => f()).catch(() => {});
    };
  });

  const last = $derived(samples.at(-1));
  const series = <T,>(pick: (s: Sample) => T) => samples.map(pick);

  const sumDisks = (s: Sample, key: "read_bps" | "write_bps") =>
    s.disks.some((d) => d[key] !== null) ? s.disks.reduce((sum, d) => sum + (d[key] ?? 0), 0) : null;

  const pct = (p: number | null | undefined) => (p == null ? "—" : formatPercent(p / 100));
  const rate = (r: number | null | undefined) => (r == null ? "—" : formatRate(r));

  const cpuSubtitle = $derived.by(() => {
    const parts: string[] = [];
    if (info?.cores) parts.push(`${info.cores} núcleos`);
    const threads = last?.cpu.cores.length || info?.threads;
    if (threads) parts.push(`${threads} threads`);
    const freq = last?.cpu.freq_avg_mhz;
    if (freq) parts.push(formatMHz(freq));
    return parts.join(" · ");
  });

  const memPressure = $derived.by(() => {
    const psi = last?.mem.psi;
    if (!psi || psi.some_avg10 <= PRESSURE_PCT) return undefined;
    return {
      text: "Sob pressão",
      hint: `Nos últimos 10 s, algum programa esperou por memória em ${formatPercent(psi.some_avg10 / 100)} do tempo.`,
    };
  });

  const usedPct = (s: Sample) => (s.mem.total ? (s.mem.used / s.mem.total) * 100 : null);
</script>

<div class="resources">
  <header>
    <h1>Recursos</h1>
    <p class="host">
      {#if info?.hostname}{info.hostname}{/if}{#if info?.distro}<span> · {info.distro}</span>{/if}
    </p>
  </header>

  {#if error}
    <section class="card notice">
      <span class="error">Não foi possível ler os recursos: {error}</span>
      <button onclick={() => attempt++}>Tentar de novo</button>
    </section>
  {:else if !last}
    <section class="card notice"><span>Lendo os recursos…</span></section>
  {:else}
    <div class="grid">
      <MetricCard title="Processador" value={pct(last.cpu.usage_pct)} subtitle={info?.cpu_model ?? undefined}>
        <p class="meta">{cpuSubtitle}<span class="temp-slot"><TempValue temp={last.cpu.temp} /></span></p>
        <Sparkline
          max={100}
          label="Uso da CPU no último minuto"
          series={[{ label: "CPU", color: "var(--res-cpu)", values: series((s) => s.cpu.usage_pct) }]}
        />
        <div class="cores" aria-label="Uso por núcleo">
          {#each last.cpu.cores as core (core.id)}
            <span
              class="core"
              title="CPU {core.id}: {pct(core.usage_pct)}{core.freq_mhz ? ` · ${formatMHz(core.freq_mhz)}` : ''}"
              style:--fill="{core.usage_pct ?? 0}%"
            ></span>
          {/each}
        </div>
      </MetricCard>

      <MetricCard
        title="Memória"
        value={formatBytes(last.mem.used)}
        subtitle="de {formatBytes(last.mem.total)} · {pct(usedPct(last))} em uso"
        badge={memPressure}
      >
        <Sparkline
          max={100}
          label="Memória em uso no último minuto"
          series={[{ label: "RAM", color: "var(--res-mem)", values: series(usedPct) }]}
        />
        <dl class="rows">
          <div>
            <dt>Disponível</dt>
            <dd>{formatBytes(last.mem.available)}</dd>
          </div>
          <div>
            <dt>Swap</dt>
            <dd>
              {#if last.mem.swap_total > 0}
                {formatBytes(last.mem.swap_used)} de {formatBytes(last.mem.swap_total)}
              {:else}
                sem swap
              {/if}
            </dd>
          </div>
          {#if last.mem.zram}
            {@const z = last.mem.zram}
            <div>
              <dt>zRAM</dt>
              <dd>
                {formatBytes(z.orig_bytes)} em {formatBytes(z.mem_used)}{#if z.compr_bytes > 0}
                  ({(z.orig_bytes / z.compr_bytes).toLocaleString("pt-BR", { maximumFractionDigits: 1 })}×){/if}
              </dd>
            </div>
          {/if}
        </dl>
      </MetricCard>

      <MetricCard
        title="Disco"
        value="↓ {rate(sumDisks(last, 'read_bps'))}"
        subtitle="↑ {rate(sumDisks(last, 'write_bps'))} gravando"
      >
        <Sparkline
          floor={1e6}
          format={formatRate}
          label="Leitura e gravação no último minuto"
          series={[
            { label: "Leitura", color: "var(--res-disk-read)", values: series((s) => sumDisks(s, "read_bps")) },
            { label: "Gravação", color: "var(--res-disk-write)", values: series((s) => sumDisks(s, "write_bps")) },
          ]}
        />
        <p class="legend">
          <span style:--dot="var(--res-disk-read)">Leitura</span>
          <span style:--dot="var(--res-disk-write)">Gravação</span>
        </p>
        {#if last.disks.length === 0}
          <p class="meta">Nenhum disco físico encontrado.</p>
        {:else}
          <ul class="list">
            {#each last.disks as d (d.name)}
              <li>
                <span class="name">
                  {d.model ?? d.name}
                  <small>{d.name} · {diskKind(d.kind, d.rotational)}</small>
                </span>
                <span class="nums">↓ {rate(d.read_bps)}<br />↑ {rate(d.write_bps)}</span>
                <span class="busy">
                  <span title="Tempo com leitura ou gravação em andamento">{pct(d.busy_pct)}</span>
                  <TempValue temp={d.temp} />
                </span>
              </li>
            {/each}
          </ul>
          {@const hint = last.disks.find((d) => d.temp_hint)?.temp_hint}
          {#if hint}
            <p class="meta hint">
              Para ver a temperatura do disco SATA, carregue o módulo do sensor (vale até reiniciar):
            </p>
            <CommandLine command={hint} />
          {/if}
        {/if}
      </MetricCard>

      <MetricCard title="Rede" value="↓ {rate(last.net.rx_bps)}" subtitle="↑ {rate(last.net.tx_bps)} enviando">
        <Sparkline
          floor={1e5}
          format={formatRate}
          label="Download e upload no último minuto"
          series={[
            { label: "Download", color: "var(--res-net-rx)", values: series((s) => s.net.rx_bps) },
            { label: "Upload", color: "var(--res-net-tx)", values: series((s) => s.net.tx_bps) },
          ]}
        />
        <p class="legend">
          <span style:--dot="var(--res-net-rx)">Download</span>
          <span style:--dot="var(--res-net-tx)">Upload</span>
        </p>
        <dl class="rows">
          <div>
            <dt>Desde que a aba abriu</dt>
            <dd>↓ {formatBytes(last.net.rx_session)} · ↑ {formatBytes(last.net.tx_session)}</dd>
          </div>
        </dl>
        {#if last.net.interfaces.length === 0}
          <p class="meta">Nenhuma interface de rede física.</p>
        {:else}
          <ul class="list">
            {#each last.net.interfaces as i (i.name)}
              <li>
                <span class="name">{i.wireless ? "Wi-Fi" : "Cabo"} <small>{i.name}</small></span>
                <span class="nums">↓ {rate(i.rx_bps)}<br />↑ {rate(i.tx_bps)}</span>
              </li>
            {/each}
          </ul>
        {/if}
      </MetricCard>

      {#if last.gpus.length > 0}
        <GpuCard {samples} />
      {/if}

      {#if last.battery_total && last.batteries.length > 0}
        <BatteryCard batteries={last.batteries} total={last.battery_total} />
      {/if}

      {#if info}
        <SystemInfoCard {info} uptime={last.uptime_s} />
      {/if}
    </div>
  {/if}
</div>

<style>
  header {
    margin-bottom: 16px;
  }

  h1 {
    margin: 0;
    font-size: 28px;
    font-weight: 700;
    letter-spacing: -0.01em;
  }

  .host {
    min-height: 1.4em;
    margin: 2px 0 0;
    color: var(--text-secondary);
  }

  .host span {
    font-size: 13px;
  }

  /* 1 coluna na janela estreita, 2 na largura padrão. */
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(100%, 300px), 1fr));
    gap: 14px;
  }

  .card {
    background: var(--surface);
    border-radius: var(--radius);
    box-shadow: var(--shadow);
  }

  .notice {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 16px 18px;
    color: var(--text-secondary);
  }

  .notice button {
    margin-left: auto;
    padding: 5px 14px;
    border: none;
    border-radius: 999px;
    background: color-mix(in srgb, var(--accent) 14%, transparent);
    color: var(--accent);
    font-weight: 600;
    font-size: 13px;
    cursor: pointer;
  }

  .error {
    color: var(--danger);
  }

  .meta {
    margin: -4px 0 4px;
    color: var(--text-secondary);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }

  /* Uma mini-barra por thread; quebram em linhas para caber de 2 a 64. */
  .cores {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(10px, 1fr));
    gap: 3px;
    margin-top: 6px;
  }

  .core {
    height: 22px;
    border-radius: 3px;
    background: linear-gradient(to top, var(--res-cpu) var(--fill), var(--track) var(--fill));
  }

  .legend {
    display: flex;
    gap: 14px;
    margin: 0;
    color: var(--text-secondary);
    font-size: 12px;
  }

  .legend span::before {
    content: "";
    display: inline-block;
    width: 8px;
    height: 8px;
    margin-right: 5px;
    border-radius: 50%;
    background: var(--dot);
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

  .list {
    margin: 4px 0 0;
    padding: 0;
    list-style: none;
  }

  .list li {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 0;
    border-top: 1px solid var(--separator);
    font-size: 13px;
  }

  .name {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  small {
    color: var(--text-secondary);
    font-size: 11px;
  }

  .nums {
    font-size: 12px;
    text-align: right;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
  }

  .temp-slot {
    float: right;
  }

  .hint {
    margin: 6px 0 0;
  }

  .busy {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    min-width: 44px;
    color: var(--text-secondary);
    font-size: 12px;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
</style>
