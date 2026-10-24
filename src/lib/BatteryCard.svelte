<!-- Card Bateria da aba Recursos: só aparece com bateria. Nível total (ponderado pela energia), estado,
     potência e tempo restante; saúde e ciclos de cada bateria. -->
<script lang="ts">
  import type { BatterySample, BatteryTotal } from "./api";
  import { formatDuration, formatPercent } from "./format";
  import MetricCard from "./MetricCard.svelte";
  import { BATTERY_STATUS_LABELS } from "./resources";

  let { batteries, total }: { batteries: BatterySample[]; total: BatteryTotal } = $props();

  /** Abaixo disso, a barra fica vermelha. */
  const LOW_PCT = 20;
  /** Saúde abaixo disso ganha destaque: a bateria já perdeu boa parte da capacidade. */
  const WORN_PCT = 60;

  const pct = (p: number | null) => (p === null ? "—" : formatPercent(p / 100));
  const watts = (w: number) => `${w.toLocaleString("pt-BR", { maximumFractionDigits: 1, minimumFractionDigits: 1 })} W`;

  const subtitle = $derived.by(() => {
    const parts = [BATTERY_STATUS_LABELS[total.status]];
    if (total.time_left_s !== null) {
      const left = formatDuration(total.time_left_s);
      parts.push(total.status === "charging" ? `cheia em ${left}` : `${left} restantes`);
    }
    return parts.join(" · ");
  });

  const powerLabel = $derived(total.status === "charging" ? "Carregando a" : "Consumo");
  // Na tomada e cheia, a potência é só ruído do medidor.
  const showPower = $derived(total.power_w !== null && total.power_w >= 0.1);
</script>

<MetricCard title="Bateria" value={pct(total.pct)} {subtitle}>
  <div
    class="bar"
    class:low={total.pct !== null && total.pct < LOW_PCT && total.status !== "charging"}
    style:--fill="{total.pct ?? 0}%"
    role="meter"
    aria-label="Nível da bateria"
    aria-valuemin={0}
    aria-valuemax={100}
    aria-valuenow={total.pct ?? 0}
  ></div>
  <dl class="rows">
    {#if showPower}
      <div>
        <dt>{powerLabel}</dt>
        <dd>{watts(total.power_w!)}</dd>
      </div>
    {/if}
    {#each batteries as b (b.name)}
      <div>
        <dt>
          Saúde{#if batteries.length > 1}<small> · {b.name}</small>{/if}
        </dt>
        <dd>
          <span class:worn={b.health_pct !== null && b.health_pct < WORN_PCT}
            title="Capacidade atual em relação à de fábrica">{pct(b.health_pct)}</span
          >{#if b.cycles !== null}<small> · {b.cycles.toLocaleString("pt-BR")} ciclos</small>{/if}
        </dd>
      </div>
    {/each}
  </dl>
</MetricCard>

<style>
  .bar {
    height: 10px;
    margin: 2px 0 6px;
    border-radius: 5px;
    background: linear-gradient(to right, var(--res-mem) var(--fill), var(--track) var(--fill));
  }

  .bar.low {
    background: linear-gradient(to right, var(--danger) var(--fill), var(--track) var(--fill));
  }

  .rows {
    margin: 0;
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

  small {
    color: var(--text-secondary);
    font-size: 11px;
  }

  .worn {
    color: var(--warning);
    font-weight: 600;
  }
</style>
