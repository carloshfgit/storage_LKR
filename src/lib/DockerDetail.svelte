<!-- Detalhe de um daemon Docker: imagens, containers, volumes e build cache item a item, com
     dicas de como liberar espaço pelo terminal. Só leitura: as únicas ações do app continuam
     sendo os botões do card Docker (DECISIONS D18, D42). -->
<script lang="ts">
  import { dockerDetails, type DockerDetails, type DockerMode, type ScanResult } from "./api";
  import CommandLine from "./CommandLine.svelte";
  import { formatBytes, formatCount, formatRelativeDate } from "./format";

  let {
    mode,
    result,
    version,
    onback,
  }: {
    mode: DockerMode;
    result: ScanResult;
    /** Muda a cada scan concluído: a lista é recarregada. */
    version: number;
    onback: () => void;
  } = $props();

  type Tab = "volumes" | "images" | "containers" | "build_cache";
  /** Uma linha da lista: o mesmo formato para os quatro tipos. */
  type Row = { key: string; name: string; title?: string; badge?: string; sub: string; size: number };
  /** Uma dica: texto e, se houver, o comando para copiar. */
  type Tip = { text: string; command?: string };

  /** Linhas mostradas antes do "Mostrar todos"; o build cache passa de 200 entradas. */
  const PAGE = 50;
  const DAY = 86_400;

  let details = $state.raw<DockerDetails | null>(null);
  let loadError = $state("");
  let tab = $state<Tab | null>(null);
  let showAll = $state(false);

  $effect(() => {
    const requested = mode;
    version;
    details = null;
    loadError = "";
    dockerDetails(requested).then(
      (d) => {
        if (requested !== mode) return;
        details = d;
        // Abre no tipo que mais ocupa.
        tab ??= (Object.keys(TABS) as Tab[]).reduce((best, t) => (sum(d[t]) > sum(d[best]) ? t : best));
      },
      (err) => (loadError = String(err)),
    );
  });

  const usage = $derived(result.docker[mode]);
  const title = $derived(mode === "rootless" ? "Docker rootless" : "Docker");
  /** O rootless tem outro socket; sem ele, o `docker` falaria com o do sistema. Pela variável,
   *  e não por `--host`, para valer também nos plugins (`compose`, `buildx`). */
  const docker = $derived(mode === "rootless" ? "DOCKER_HOST=unix://$XDG_RUNTIME_DIR/docker.sock docker" : "docker");

  const TABS: Record<Tab, string> = {
    volumes: "Volumes",
    images: "Imagens",
    containers: "Containers",
    build_cache: "Build cache",
  };

  const sum = (items: { size: number }[]) => items.reduce((s, i) => s + i.size, 0);
  /** No Docker 23+, `volume prune` sem `-a` só apaga anônimos; antes, apagava também os com nome. */
  const safePrune = $derived(Number(details?.server_version?.split(".")[0]) >= 23);
  const plural = (n: number, one: string, many: string) => `${formatCount(n)} ${n === 1 ? one : many}`;
  const short = (id: string) => id.replace(/^sha256:/, "").slice(0, 12);
  const STATES: Record<string, string> = {
    running: "rodando",
    exited: "parado",
    created: "criado, nunca iniciado",
    paused: "pausado",
    restarting: "reiniciando",
    dead: "morto",
    removing: "sendo removido",
  };

  const rows = $derived.by((): Row[] => {
    if (!details || !tab) return [];
    switch (tab) {
      case "volumes":
        return details.volumes.map((v) => ({
          key: v.name,
          name: v.anonymous ? short(v.name) : v.name,
          title: v.name,
          badge: v.anonymous ? "anônimo" : undefined,
          sub: [
            v.links === 0 ? "sem uso" : `em uso por ${plural(v.links, "container", "containers")}`,
            v.project && `projeto ${v.project}`,
          ]
            .filter(Boolean)
            .join(" · "),
          size: v.size,
        }));
      case "images":
        return details.images.map((i) => {
          const dangling = i.repository === "<none>";
          return {
            key: i.id,
            name: dangling ? short(i.id) : i.tag && i.tag !== "<none>" ? `${i.repository}:${i.tag}` : i.repository,
            title: i.id,
            badge: dangling ? "sem nome" : undefined,
            sub: [
              i.containers === 0 ? "sem container" : `usada por ${plural(i.containers, "container", "containers")}`,
              i.created > 0 && `criada ${formatRelativeDate(i.created)}`,
              i.shared_size > 0 && `${formatBytes(i.shared_size)} compartilhados`,
            ]
              .filter(Boolean)
              .join(" · "),
            size: i.size,
          };
        });
      case "containers":
        return details.containers.map((c) => ({
          key: c.id,
          name: c.name,
          sub: [c.image, STATES[c.state] ?? c.state, c.project && `projeto ${c.project}`].filter(Boolean).join(" · "),
          size: c.size,
        }));
      case "build_cache":
        return details.build_cache.map((b) => ({
          key: b.id,
          name: b.description || short(b.id),
          title: b.description,
          badge: b.in_use ? "em uso" : undefined,
          sub: [
            b.last_used > 0 && `último uso ${formatRelativeDate(b.last_used)}`,
            plural(b.usage_count, "uso", "usos"),
            b.shared && "compartilhado",
          ]
            .filter(Boolean)
            .join(" · "),
          size: b.size,
        }));
    }
  });

  const visible = $derived(showAll ? rows : rows.slice(0, PAGE));

  /** Dicas do tipo aberto, montadas a partir do que existe nele. */
  const tips = $derived.by((): Tip[] => {
    if (!details || !tab) return [];
    const now = Date.now() / 1000;
    const tips: Tip[] = [];
    switch (tab) {
      case "volumes": {
        const anon = details.volumes.filter((v) => v.anonymous && v.links === 0);
        const named = details.volumes.filter((v) => !v.anonymous && v.links === 0);
        if (anon.length > 0) {
          const intro =
            `${plural(anon.length, "volume anônimo está", "volumes anônimos estão")} sem uso (${formatBytes(sum(anon))}). ` +
            "Eles sobram quando um container é recriado e quase nunca guardam algo importante. ";
          tips.push(
            safePrune
              ? {
                  text: intro + `Neste Docker (${details.server_version}), este comando remove só os anônimos sem uso:`,
                  command: `${docker} volume prune`,
                }
              : {
                  // Antes da 23, o prune levaria junto os volumes com nome: filtra pelo id.
                  text:
                    intro +
                    `${details.server_version ? `Neste Docker (${details.server_version})` : "Nesta versão do Docker"}, ` +
                    "o docker volume prune apagaria também os volumes com nome. Este comando remove só os sem uso com nome de id:",
                  // `xargs` não aceita `VAR=…` antes do comando: no rootless, a variável é exportada.
                  command:
                    (mode === "rootless" ? "export DOCKER_HOST=unix://$XDG_RUNTIME_DIR/docker.sock; " : "") +
                    "docker volume ls -qf dangling=true | grep -E '^[0-9a-f]{64}$' | xargs -r docker volume rm",
                },
          );
        }
        if (named.length > 0) {
          tips.push({
            text:
              `${plural(named.length, "volume com nome está", "volumes com nome estão")} sem uso (${formatBytes(sum(named))}). ` +
              "Esses costumam guardar dados (bancos, uploads): confira o nome e remova um por vez, só o que tiver certeza:",
            command: `${docker} volume rm <nome>`,
          });
        }
        tips.push({
          text: "Para um projeto do Compose que você não usa mais, rode na pasta dele. Remove os containers e os volumes do projeto:",
          command: `${docker} compose down -v`,
        });
        tips.push({ text: "Um volume em uso só pode ser removido depois do container que o monta." });
        break;
      }
      case "images": {
        const unused = details.images.filter((i) => i.containers === 0);
        const dangling = unused.filter((i) => i.repository === "<none>");
        if (dangling.length > 0) {
          tips.push({
            text: `${plural(dangling.length, "imagem sem nome é sobra", "imagens sem nome são sobras")} de builds antigos (${formatBytes(sum(dangling))}). Remova só essas com:`,
            command: `${docker} image prune`,
          });
        }
        if (unused.length > 0) {
          tips.push({
            text:
              `${plural(unused.length, "imagem não é usada", "imagens não são usadas")} por nenhum container (${formatBytes(sum(unused))}). ` +
              'O botão "Remover imagens não usadas" faz o mesmo que este comando; elas são baixadas de novo quando precisar:',
            command: `${docker} image prune -a`,
          });
        }
        tips.push({ text: "Para remover uma imagem específica:", command: `${docker} rmi <repositório:tag>` });
        tips.push({
          text: "O tamanho de cada imagem inclui camadas compartilhadas com outras; remover uma libera só a parte que é só dela.",
        });
        break;
      }
      case "containers": {
        const stopped = details.containers.filter((c) => c.state !== "running" && c.state !== "paused");
        if (stopped.length > 0) {
          tips.push({
            text:
              `${plural(stopped.length, "container está parado", "containers estão parados")} (${formatBytes(sum(stopped))}). ` +
              "Ao remover, perde-se o que foi gravado dentro dele; o que está em volumes fica. Para remover todos os parados:",
            command: `${docker} container prune`,
          });
        }
        tips.push({ text: "Para remover um container específico:", command: `${docker} rm <nome>` });
        tips.push({ text: "Os de um projeto do Compose, na pasta do projeto:", command: `${docker} compose down` });
        tips.push({
          text: "Remover o container também libera a imagem dele para o \"Remover imagens não usadas\".",
        });
        break;
      }
      case "build_cache": {
        const old = details.build_cache.filter((b) => !b.in_use && b.last_used > 0 && now - b.last_used > 30 * DAY);
        if (old.length > 0) {
          tips.push({
            text: `${plural(old.length, "entrada não é usada", "entradas não são usadas")} há mais de 30 dias (${formatBytes(sum(old))}). Remova só essas, mantendo o cache recente:`,
            command: `${docker} builder prune --filter until=720h`,
          });
        }
        tips.push({
          text:
            'Para remover todo o cache que não está em uso (é o que o botão "Limpar build cache" faz). ' +
            "Os próximos builds ficam mais lentos, porque refazem esses passos:",
          command: `${docker} builder prune`,
        });
        break;
      }
    }
    return tips;
  });

  function select(t: Tab) {
    tab = t;
    showAll = false;
  }

</script>

<svelte:window onkeydown={(e) => e.key === "Escape" && onback()} />

<button class="back" onclick={onback}>
  <svg viewBox="0 0 16 16" aria-hidden="true"><path d="M10 3 5 8l5 5" /></svg>
  Apps/Sistema
</button>

<header>
  <h1>{title}</h1>
  {#if usage}
    <p>
      <strong>{formatBytes(usage.images.size + usage.containers.size + usage.volumes.size + usage.build_cache.size)}</strong>
      · {formatBytes(
        usage.images.reclaimable + usage.containers.reclaimable + usage.volumes.reclaimable + usage.build_cache.reclaimable,
      )} liberáveis
    </p>
  {/if}
</header>

{#if loadError}
  <section class="card"><p class="error">Não foi possível consultar o Docker: {loadError}</p></section>
{:else if !details || !tab}
  <section class="card"><p class="muted">Consultando o Docker… (leva alguns segundos)</p></section>
{:else}
  <div class="segmented tabs" role="tablist" aria-label="Tipo">
    {#each Object.entries(TABS) as [key, label] (key)}
      {@const items = details[key as Tab]}
      <button role="tab" aria-selected={tab === key} class:active={tab === key} onclick={() => select(key as Tab)}>
        {label} <span>{formatCount(items.length)} · {formatBytes(sum(items))}</span>
      </button>
    {/each}
  </div>

  <section class="card tips">
    <h2>Como liberar espaço</h2>
    {#if mode === "rootless"}
      <p class="muted">Os comandos apontam para o socket do daemon rootless.</p>
    {/if}
    <ul>
      {#each tips as tip (tip.text)}
        <li>
          <p>{tip.text}</p>
          {#if tip.command}<CommandLine command={tip.command} />{/if}
        </li>
      {/each}
    </ul>
  </section>

  <section class="card">
    {#if rows.length === 0}
      <p class="muted">Nada aqui.</p>
    {:else}
      <ul class="items">
        {#each visible as row (row.key)}
          <li>
            <div class="text">
              <span class="name" title={row.title ?? row.name}>
                {row.name}{#if row.badge}<span class="badge">{row.badge}</span>{/if}
              </span>
              <span class="sub" title={row.sub}>{row.sub}</span>
            </div>
            <span class="size">{formatBytes(row.size)}</span>
          </li>
        {/each}
      </ul>
      {#if rows.length > PAGE}
        <button class="more" onclick={() => (showAll = !showAll)}>
          {showAll ? "Mostrar menos" : `Mostrar todos (${formatCount(rows.length)})`}
        </button>
      {/if}
    {/if}
    <p class="footnote">Tamanhos informados pelo Docker. A mesma lista aparece no terminal com docker system df -v.</p>
  </section>
{/if}

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

  h2 {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
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

  .segmented {
    display: flex;
    padding: 2px;
    border-radius: 8px;
    background: var(--track);
  }

  .tabs {
    margin-bottom: 14px;
  }

  .segmented button {
    display: flex;
    flex: 1;
    flex-direction: column;
    align-items: center;
    padding: 4px 8px;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: var(--text);
    font-size: 13px;
    cursor: pointer;
    transition: background var(--duration) var(--ease);
  }

  .segmented button span {
    color: var(--text-secondary);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }

  .segmented button.active {
    background: var(--surface);
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.12);
    font-weight: 600;
  }

  .tips p {
    margin: 0;
    font-size: 13px;
  }

  .tips > .muted {
    margin-top: 4px;
  }

  .tips li {
    padding: 8px 0;
    border-top: 1px solid var(--separator);
  }

  .tips li:first-child {
    border-top: none;
  }

  .more {
    flex: none;
    padding: 3px 12px;
    border: none;
    border-radius: 999px;
    background: color-mix(in srgb, var(--accent) 14%, transparent);
    color: var(--accent);
    font-size: 12px;
    font-weight: 600;
    cursor: pointer;
  }

  .more {
    margin-top: 8px;
  }

  .items li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
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
  .sub {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .sub {
    color: var(--text-secondary);
    font-size: 12px;
  }

  .badge {
    margin-left: 6px;
    padding: 1px 6px;
    border-radius: 999px;
    background: var(--track);
    color: var(--text-secondary);
    font-size: 11px;
  }

  .size {
    flex: none;
    font-weight: 500;
    font-variant-numeric: tabular-nums;
  }
</style>
