<!-- Card Ficha técnica da aba Recursos: o que não muda, mais o tempo ligado (a cada amostra). -->
<script lang="ts">
  import type { GpuSpec, SystemInfo } from "./api";
  import { formatBytes, formatDuration, formatMHz } from "./format";
  import MetricCard from "./MetricCard.svelte";
  import { diskKind, gpuKind, gpuName } from "./resources";

  let { info, uptime }: { info: SystemInfo; uptime: number | null } = $props();

  /** "dedicada · amdgpu · 4,29 GB"; sem nome no `pci.ids`, o `vendor:device` para pesquisar. */
  const gpuDetails = (g: GpuSpec) =>
    [gpuKind(g), g.driver, g.vram_total ? formatBytes(g.vram_total) : "", g.model ? "" : g.pci_id]
      .filter(Boolean)
      .join(" · ");

  /** "2 núcleos · 4 threads"; numa VM, mais a frequência nominal (a amostra vem sem frequência). */
  const cores = $derived(
    [
      info.cores ? `${info.cores} núcleos` : "",
      `${info.threads} threads`,
      info.cpu_nominal_mhz ? `${formatMHz(info.cpu_nominal_mhz)} nominal` : "",
    ]
      .filter(Boolean)
      .join(" · "),
  );
</script>

<MetricCard title="Ficha técnica" value={info.distro ?? "Linux"} subtitle={info.hostname ?? undefined}>
  <dl class="rows">
    <div>
      <dt>Processador</dt>
      <dd>{info.cpu_model ?? "—"}<small>{cores}</small></dd>
    </div>
    <div>
      <dt>Memória</dt>
      <dd>{formatBytes(info.ram_total)}</dd>
    </div>
    {#each info.disks as d (d.name)}
      <div>
        <dt>Disco</dt>
        <dd>{d.model ?? d.name}<small>{formatBytes(d.size_bytes)} · {diskKind(d.kind, d.rotational)}</small></dd>
      </div>
    {/each}
    {#each info.gpus as g (g.card)}
      <div>
        <dt>GPU</dt>
        <dd>{gpuName(g)}<small>{gpuDetails(g)}</small></dd>
      </div>
    {/each}
    <div>
      <dt>Kernel</dt>
      <dd>{info.kernel ?? "—"}</dd>
    </div>
    {#if uptime !== null}
      <div>
        <dt>Ligado há</dt>
        <dd>{formatDuration(uptime)}</dd>
      </div>
    {/if}
  </dl>
</MetricCard>

<style>
  .rows {
    margin: 0;
  }

  .rows div {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 12px;
    padding: 5px 0;
    border-top: 1px solid var(--separator);
    font-size: 13px;
  }

  dt {
    flex-shrink: 0;
    color: var(--text-secondary);
  }

  dd {
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    min-width: 0;
    margin: 0;
    text-align: right;
    overflow-wrap: anywhere;
  }

  small {
    color: var(--text-secondary);
    font-size: 11px;
  }
</style>
