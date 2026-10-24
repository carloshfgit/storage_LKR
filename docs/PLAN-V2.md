# PLAN V2 — Aba "Recursos"

Plano de implementação da ideia em [ideia-recursos.md](ideia-recursos.md): uma segunda aba, **Recursos**, com a telemetria do computador em tempo real (CPU, memória, disco, GPU, rede, processos e ficha técnica; a bateria saiu na revisão da Etapa 19, D56), ao lado da aba **Armazenamento**, que já existe.

A numeração continua a do [PLAN.md](PLAN.md) (Etapas 15 a 20), para manter os commits no padrão `feat: etapa N - <descrição>`. As decisões previstas abaixo (D45 a D52) entram no [DECISIONS.md](DECISIONS.md) quando forem implementadas, com o resultado medido. O "como funciona" vai para o [ARCHITECTURE.md](ARCHITECTURE.md) (nova §11) e o progresso para a tabela de status do README.

**Escopo da V2:**
- Seletor centralizado no topo: **Armazenamento | Recursos**.
- Cards com gráfico dos últimos 60 s: CPU (total, por núcleo, frequência, temperatura), Memória (RAM, swap/zRAM, pressão), Disco (leitura/gravação em MB/s, temperatura), GPU (uso, VRAM, temperatura, consumo) e Rede (download/upload e total da sessão).
- "Top 5" processos por CPU, RAM ou disco.
- Ficha técnica, na linha inteira do grid (o card Bateria saiu na revisão da Etapa 19, D56).
- Coleta só com a aba visível: sai da aba ou minimiza a janela, a coleta para.

**Compatibilidade (D52):** o app roda em qualquer Ubuntu suportado, não só na máquina de desenvolvimento.
- Mínimo: **Ubuntu 22.04 LTS** (kernel 5.15) e derivados (Kubuntu, Xubuntu, Pop!_OS, Mint 21+), em X11 e Wayland. Tudo o que o plano lê já existe nesse kernel (PSI, hwmon de NVMe, `drivetemp`, `gpu_busy_percent`).
- Arquitetura do pacote: `amd64`. Os parsers não assumem x86: os casos de ARM (`cpu_thermal`, `devicetree`) ficam cobertos por testes, para um `.deb` `arm64` futuro.
- Nenhum critério de "Pronto quando" depende de um hardware específico. A máquina de desenvolvimento é só um dos casos medidos no "Resultado"; os outros formatos são cobertos por árvores falsas nos testes (ver "Matriz de validação").
- Nenhuma ferramenta externa é obrigatória: `lspci` e `nvidia-smi` são opcionais, e a falta deles é um caso normal, testado.
- Telas de 1366×768 a 4K (HiDPI com escala 200%).

**Fora do escopo:** histórico além da sessão (nada é gravado em disco), alertas e notificações, encerrar processos pelo app (a lista é só de leitura, como o detalhe do Docker em D42), métricas que exigem root (RAPL da CPU, I/O de processos de outros usuários, uso da GPU Intel).

---

## Visão geral da arquitetura

```
┌──────────────────────────── Frontend (Svelte 5) ──────────────────────────────────────┐
│  +page.svelte ─ TabSwitch ─┬─ (Armazenamento: tela atual, escondida, não desmontada)  │
│                            └─ ResourcesView ─ MetricCard×N (Sparkline SVG) ─ TopProc. │
│        ▲ MonitorEvent (Channel, 1 Hz)               │ start_monitor / stop_monitor     │
└────────┼────────────────────────────────────────────┼─────────────────────────────────┘
         │                                            ▼
┌──────────────────────────── src-tauri ────────────────────────────────────────────────┐
│  state/monitor.rs: MonitorState { running, generation } — thread com tick de 1 s,     │
│  para sozinha se o Channel falhar ou chegar outra geração                             │
└────────┬──────────────────────────────────────────────────────────────────────────────┘
         ▼
┌──────────────────────────── crates/monitor (Rust puro, sem Tauri) ────────────────────┐
│  Sampler { root, anterior } → sample() -> Sample      system_info() -> SystemInfo     │
│  cpu.rs · mem.rs · disk_io.rs · net.rs · sensors.rs · gpu/ · procs.rs                 │
│  Leitura direta de /proc e /sys; parsers puros sobre o texto dos arquivos             │
└───────────────────────────────────────────────────────────────────────────────────────┘
```

**Princípios:**
- **Crate próprio, `crates/monitor/`**, separado do `scanner`: nada em comum além do Rust, e assim cada um testa e mede sem o outro. Sem dependência do Tauri; `bin/monitor.rs` imprime amostras no terminal para conferir os números com `top`, `free`, `iostat`, `sensors` e `nvidia-smi`, como o `scan` confere com `df`/`du`.
- **Leitura direta de `/proc` e `/sys`, sem `sysinfo`** (D45). Cada coletor tem um parser puro (`fn parse_stat(&str) -> …`) e o `Sampler` recebe a raiz (`/` em produção, uma pasta temporária com uma árvore falsa de `proc/` e `sys/` nos testes).
- **Taxas calculadas no Rust.** O `Sampler` guarda os contadores da amostra anterior (jiffies da CPU, setores do `diskstats`, bytes do `net/dev`, tempos de cada processo) e manda a diferença dividida pelo tempo real decorrido (`Instant`), não pelo intervalo nominal. A primeira amostra vem sem taxas (`None`).
- **Histórico no frontend.** O backend não guarda série: a UI mantém um anel de 60 amostras. Voltar para a aba recomeça o gráfico (D47).
- **"Não disponível" é um valor, não um erro.** Todo campo que depende de hardware ou driver é `Option`. Card sem nenhum dado some; campo sem dado mostra "—" com uma dica quando houver uma solução conhecida (ex.: `modprobe drivetemp` para a temperatura de SSD SATA).
- **Nada de root.** O que exige root fica de fora (lista em "Fora do escopo"), na mesma linha de D19.

**Modelo (rascunho, `crates/monitor/src/model.rs`):**

```rust
#[derive(Serialize)]
struct Sample {
    at_ms: u64,                       // relógio monotônico, para o eixo X
    cpu: CpuSample,                   // total_pct, per_core_pct: Vec<f32>, freq_mhz (média e máx), temp_c
    mem: MemSample,                   // total, used, available, swap_total, swap_used, zram: Option<Zram>, psi_some_avg10
    disks: Vec<DiskIo>,               // name, model, kind (nvme|sata|emmc|virtio|…), read_bps, write_bps, temp_c (só discos físicos inteiros)
    net: NetSample,                   // rx_bps, tx_bps, rx_session, tx_session (só interfaces físicas)
    gpus: Vec<GpuSample>,             // name, vendor, busy_pct, vram_used/total, temp_c, power_w, freq_mhz
    procs: Option<Vec<ProcSample>>,   // a cada 2 ticks; None nos outros
    uptime_s: u64,
}

#[derive(Serialize)]
struct SystemInfo {                   // uma vez, ao abrir a aba
    cpu_model, cores, threads, ram_total,
    gpus: Vec<GpuInfo>, disks: Vec<DiskInfo>, kernel, distro, hostname,
}
```

**Eventos (`src-tauri`):**

```rust
#[serde(tag = "kind", rename_all = "snake_case")]
enum MonitorEvent {
    Sample { sample: Box<Sample> },
    Stopped,                          // fim normal (stop_monitor ou outra geração)
    Error { message: String },
}
```

---

## Etapa 15 — Motor de telemetria (CLI)
Base do crate `monitor`, só com o que vem de `/proc` e é igual em qualquer máquina: CPU, memória, disco e rede.
- [x] Criar `crates/monitor/` e incluí-lo no workspace; perfil `dev` com `opt-level = 3` para ele (como o `scanner`).
- [x] `cpu.rs`: `/proc/stat` (linha `cpu` e `cpuN`), uso = 1 − Δ(idle + iowait) / Δtotal. Frequência por `/sys/devices/system/cpu/cpu*/cpufreq/scaling_cur_freq` (média e máxima). Sem `cpufreq` (VM), sem frequência na amostra; o `cpu MHz` do `/proc/cpuinfo` vai para a ficha técnica como nominal (revisão, D55).
- [x] `mem.rs`: `/proc/meminfo`. Usada = `MemTotal − MemAvailable` (como o `free` e o GNOME). Swap em disco pelo `/proc/swaps`, sem os `/dev/zram*` (revisão, D54; antes, `SwapTotal/SwapFree`, que inclui o zRAM); zRAM de `/sys/block/zram*/mm_stat` (dados originais e comprimidos, para mostrar a taxa de compressão). Pressão de `/proc/pressure/memory` (`some avg10`), quando o kernel tiver PSI.
- [x] `disk_io.rs`: `/proc/diskstats`, só **discos físicos inteiros**: os nomes que existem em `/sys/block` (as partições não aparecem lá) e têm `device/`. Isso inclui `sd*` (SATA/USB), `nvme*n*`, `mmcblk*` (eMMC e cartão SD, sem `mmcblk*boot*` nem `*rpmb`), `vd*`/`xvd*` (VMs). Ficam de fora `loop*`, `ram*`, `zram*`, `sr*`, e também `dm-*` (LVM, LUKS) e `md*` (RAID), que repetem o I/O dos discos físicos embaixo e o contariam duas vezes. Bytes = setores × 512 (o `diskstats` sempre conta em 512, independente do setor físico). Modelo em `device/model` (ou `device/name` no eMMC); tipo pelo nome e por `queue/rotational` (HD/SSD).
- [x] `net.rs`: `/proc/net/dev`, só interfaces físicas (com `/sys/class/net/<i>/device`). Fica de fora `lo`, `docker0`, `br-*`, `veth*`, `virbr*`, `tun*`/`wg*` (VPN contaria o tráfego duas vezes). Total da sessão = soma das diferenças desde a primeira amostra, com proteção contra contador zerado (interface que caiu e voltou).
- [x] `Sampler::new(root)` + `sample()`; `uptime` de `/proc/uptime`.
- [x] `bin/monitor.rs`: `monitor` (uma linha por segundo), `--once`, `--interval <ms>`, `--json`.
- [x] Testes: parsers com textos reais (incluindo `/proc/stat` com núcleo desligado, `meminfo` sem swap, sem zRAM e com zRAM sem swap em disco, kernel sem PSI, `diskstats` com partições, `loop`, NVMe, eMMC, `md0` sobre dois `sd*` e `dm-0` sobre `nvme0n1`, `net/dev` com contador reiniciado e com Wi-Fi + cabo ao mesmo tempo, máquina sem `cpufreq`, como uma VM), taxas com dois snapshots e tempo controlado, primeira amostra sem taxas.

**Pronto quando:**
- `target/release/monitor` mostra uso de CPU a ±3 p.p. do `top` (mesmo intervalo), memória igual ao `free -b`, I/O a ±5% do `iostat -d 1` e rede a ±5% do `ifstat`/`nload`.
- Rodando `target/release/scan /` em paralelo, o monitor mostra a leitura do disco subir.
- Custo de uma amostra < 1 ms e < 0,5% de um núcleo a 1 Hz (medido por `/proc/self/stat` do próprio CLI depois de 5 min).

**Resultado (23/10/2026, Ubuntu, SSD SATA, 4 threads, Wi-Fi + cabo):**

| Medida | `monitor` | Referência |
|---|---|---|
| CPU, 12 amostras com `dd` gravando | diferença de −1,0 a +0,9 p.p. | `mpstat 1` (100 − idle − iowait) |
| Disco gravado em 12 s | 1.592 MB, pico de 482 MB/s | `iostat -d 1`: 1.594 MB, pico de 478 MB/s |
| Memória usada | 6.351.151.104 B | `free -b`: 6.351.171.584 B (20 kB de diferença entre as duas leituras) |
| Rede (Wi-Fi) | ↓ 71,7 kB/s, ↑ 4,7 kB/s | `sar -n DEV 1`: ↓ 71,8 kB/s, ↑ 4,7 kB/s |
| `scan /` em paralelo | leitura de `sda` sobe para 47–65 MB/s, ocupado 50–64% | — |
| Custo de uma amostra, laço quente | ≈ 270 µs | meta < 1 ms |
| Custo a 1 Hz por 5 min (300 amostras) | média 1,07 ms, pior 11,3 ms; **0,10% de um núcleo** | meta < 0,5% |

- 29 testes no crate (`cargo test -p monitor`), workspace inteiro passando, clippy limpo.
- A primeira versão lia o `/proc` com `fs::read_to_string` e levava 1,7–1,9 ms por amostra: o procfs informa tamanho 0, e a leitura começava com 32 bytes e ia em pedaços. Com a leitura única num buffer reaproveitado, e as interfaces de rede em cache como os discos, o laço quente caiu para cerca de 270 µs (D45).
- A 1 Hz, a média (1,07 ms) fica um pouco acima de 1 ms porque a CPU desta máquina volta a 800 MHz entre as amostras e cada leitura do `/proc` paga a saída do estado ocioso. A meta que importa, o custo em CPU, ficou em 0,10% de um núcleo, um quinto do limite.
- A rede foi conferida só com tráfego baixo (cerca de 70 kB/s). Falta repetir com um download grande.
- O iowait conta como ocioso, como no `top`. Comparado com `100 − idle` do `mpstat`, a diferença durante o `dd` chegava a 36 p.p.

**Revisão (24/10/2026):**
- **Swap separado do zRAM** (D54): o `SwapTotal` do `meminfo` inclui o zRAM usado como swap, e o card mostrava as mesmas páginas nas linhas "Swap" e "zRAM". Agora o swap vem do `/proc/swaps` sem os `/dev/zram*` (o `meminfo` fica como alternativa), e a linha se chama "Swap em disco" quando há zRAM. Nesta máquina, sem zRAM, continua igual ao `free -b` (4.294.963.200 B).
- **Frequência** (D55): se há `cpufreq` é decidido na primeira amostra. Sem ele (VM), a amostra vem sem frequência e não lê mais nada a cada segundo; o `cpu MHz` do `cpuinfo`, que numa VM é fixo, vai para a ficha técnica como nominal.
- 5 testes novos (102 no crate; 205 no workspace), clippy limpo, `npm run check` sem erros.

---

## Etapa 16 — Ponte Tauri e aba "Recursos"
Primeira versão visível: o seletor de abas e os cards da Etapa 15 com gráficos.
- [x] `src-tauri/src/state/monitor.rs`: `MonitorState { generation: Mutex<u64>, wake: Condvar }` (o `Condvar` faz o `stop` valer na hora, sem esperar o tick), separado do `AppState` do scan (não divide lock com ele). `start(interval, sampler_factory, send)` sobe a thread; um novo `start` incrementa a `generation`, e a thread antiga para no próximo tick. A thread para sozinha se o `send` falhar (janela fechada ou recarregada).
- [x] `system_info` (adiantado da Etapa 17, sem GPUs nem discos): modelo da CPU (ou da placa ARM pelo devicetree), núcleos físicos, threads, RAM, kernel, distribuição e hostname, com testes de x86 com SMT, dois soquetes, ARM, container e `os-release` sem `PRETTY_NAME`.
- [x] Commands `start_monitor(on_event: Channel<MonitorEvent>)`, `stop_monitor()` e `system_info()` — os 4 passos do CLAUDE.md (`commands.rs`, `lib.rs`, `build.rs`, `capabilities/default.json`) + wrappers em `api.ts` com os tipos espelhados.
- [x] O monitor **não** é recusado durante um scan: é justamente quando ver o disco trabalhando interessa. A coleta não toca no disco escaneado (só `/proc` e `/sys`, que são memória).
- [x] **Pausa** (D46): a UI chama `start_monitor` ao mostrar a aba e `stop_monitor` ao sair dela, em `visibilitychange` (`hidden`) e ao minimizar (`onResized` + `isMinimized()`; e também a cada `onFocusChanged`; o `core:default` já inclui `core:window:allow-is-minimized`). Perder o foco **não** pausa: o caso de uso é olhar o monitor enquanto outra janela faz a tarefa pesada.
- [x] `TabSwitch.svelte`: controle segmentado no topo, centralizado, estilo iOS (`role="tablist"`, setas do teclado). A aba Armazenamento fica escondida, não desmontada (a barra não refaz a animação, como na volta do detalhe). A aba escolhida é lembrada em `localStorage` (com `try/catch`), mas o app sempre abre escaneando.
- [x] `ResourcesView.svelte`: grade responsiva de cards (`grid-template-columns: repeat(auto-fill, minmax(…))`: 1 coluna na janela estreita, 2 ou 3 nas largas). Mini-barras por núcleo quebram em linhas, para caberem de 2 a 64 threads. SVG com `viewBox` e `vector-effect: non-scaling-stroke`, nítido em HiDPI. `MetricCard.svelte` (título, valor grande, subtítulo, gráfico) e `Sparkline.svelte`: SVG nativo, `<polyline>`/`<path>` com área preenchida, escala Y fixa para porcentagens e automática (com teto arredondado) para taxas; duas séries para leitura/gravação e download/upload (D48).
- [x] Cards desta etapa: **CPU** (total + mini-barras por núcleo + frequência), **Memória** (RAM com barra usada/total e swap/zRAM; selo "sob pressão" com PSI `avg10` > 10%), **Disco** (leitura/gravação por disco), **Rede** (download/upload + total da sessão).
- [x] `format.ts`: `formatRate` (B/s → kB/s → MB/s, decimal como o resto), `formatPercent` já existe, `formatMHz`, `formatTemp`, `formatDuration` (uptime e tempo de bateria).
- [x] `theme.css`: tokens `--res-cpu`, `--res-mem`, `--res-disk-read`, `--res-disk-write`, `--res-net-rx`, `--res-net-tx`, `--res-gpu`, nos dois temas.
- [x] Testes do `MonitorState` com `mpsc` e `Sampler` falso (como D24): ciclo `sample`* → `stopped`, `start` duplo (só a geração nova emite), parada quando o `send` falha.

**Pronto quando:**
- Trocar de aba não perde o estado do armazenamento (detalhe aberto, ordenação, seleção).
- Com a aba Armazenamento visível ou a janela minimizada, `top -p <pid>` mostra o app parado (sem ticks). Com a aba Recursos visível, o processo do app (Rust + WebKit) fica abaixo de 2% de um núcleo.
- Os números dos cards batem com o CLI da Etapa 15.
- A aba funciona em 1366×768 sem rolagem horizontal, e em HiDPI (escala 200%) os gráficos ficam nítidos.
- A pausa ao minimizar foi conferida em X11 e em Wayland (GNOME); a limitação, se houver, está documentada.

**Resultado (23/10/2026):**

| Medida (app + WebKit, % de um núcleo) | Valor |
|---|---|
| Aba Armazenamento, scan concluído | 0,1–0,2% (o monitor não roda) |
| Aba Recursos, `tauri dev` | ~5,5% (Rust ~1%, WebKit ~4,5%) |
| Aba Recursos, build de release | **~4%** (Rust 0,8–1,0%, WebKit 2,8–4,2%), em três medições de 30 s |

- Workspace: 138 testes passando (12 novos: 6 do ciclo do `MonitorState` e 6 da ficha técnica), clippy limpo, `npm run check` sem erros.
- Os eventos chegam a 1 Hz, numa sessão só, e a troca de aba para a coleta (a thread sai e o app volta a 0,1–0,2%).
- ⚠️ **Meta de 2% não atingida.** O release fica em cerca de 4%, e o custo está na repintura do WebKitGTK, não na coleta: o Rust inteiro (com a thread do monitor) fica abaixo de 1%. Desligar todos os gráficos SVG não mudou o custo além do ruído, nem isolar a pintura dos cards com `contain: content`. Fica para a Etapa 20 (D51): medir em outra máquina, testar a composição acelerada do WebKitGTK e decidir entre otimizar e ajustar a meta.
- Medir logo depois de recarregar a página engana: cada carga começa um scan em segundo plano (walk + Docker), que domina os primeiros 30–40 s.
- ⏳ **Falta a conferência visual**, que precisa ser feita por você:
  1. Abrir "Recursos": os quatro cards se mexem a cada segundo, e os números batem com o `target/release/monitor`.
  2. Com um detalhe aberto no Armazenamento (ordenação e seleção feitas), ir a Recursos e voltar: está tudo igual. Na aba Recursos, o `Esc` não fecha o detalhe escondido.
  3. Minimizar com a aba Recursos aberta e conferir, pelo `top -p $(pgrep -x storage-looker)`, que o app para; restaurar e ver o gráfico recomeçar. Conferir no Wayland e, se possível, numa sessão X11.
  4. Janela no mínimo (560 px): 1 coluna, sem rolagem horizontal. Escala de 200%: gráficos e mini-barras nítidos.

---

## Etapa 17 — Sensores, bateria e ficha técnica
> A bateria (`battery.rs` e o card) saiu do app na revisão da Etapa 19 (D56). O registro abaixo fica como histórico.

Tudo que depende de `/sys/class/hwmon`, `/sys/class/power_supply` e varia de máquina para máquina.
- [x] `sensors.rs`: varre `/sys/class/hwmon/hwmon*` uma vez (no `Sampler::new`) e guarda os arquivos de cada sensor; depois só lê `temp*_input`. Ordem de preferência para a **CPU**: `coretemp` (`Package id 0`), `k10temp` (`Tctl`/`Tdie`), `zenpower`, `cpu_thermal` (ARM); sem nenhum, a `thermal_zone` do tipo `x86_pkg_temp`. `acpitz` só como último recurso, marcado como aproximado.
- [x] Temperatura do **disco**: `nvme` (hwmon ligado a `/sys/block/nvmeXnY/device`) e `drivetemp` (SATA, liga o hwmon ao disco pelo `device`). SATA sem o módulo: "—" com a dica `sudo modprobe drivetemp` (o app só mostra o comando, D44). eMMC, cartão SD, USB e disco de VM: "—" sem dica (não há sensor).
- [x] `battery.rs`: **todas** as `/sys/class/power_supply/*` com `type = Battery` e `scope` diferente de `Device` (fora mouse e fone Bluetooth). Aceita os nomes `BAT*`, `CMB*` e outros: o filtro é o `type`, não o nome. Aceita os dois formatos: `energy_*` (µWh) + `power_now`, ou `charge_*` (µAh) + `current_now` × `voltage_now` (convertido para µWh com `voltage_min_design`, ou `voltage_now` sem ele). Saúde = `full / full_design`; tempo restante = energia restante / potência (descarregando) ou faltante / potência (carregando), escondido com potência ≈ 0 ou nos primeiros 10 s (valor instável). Ciclos de `cycle_count` quando ≠ 0. Com mais de uma bateria (ThinkPad com interna + externa), `battery_total` soma energia e potência, e o nível é ponderado pela energia de cada uma, como o UPower.
- [x] `system_info()` (a base veio da Etapa 16; aqui entraram os discos, com tamanho; as GPUs ficam para a 18): modelo da CPU (`/proc/cpuinfo` `model name`, ou `/sys/firmware/devicetree/base/model`), núcleos físicos e threads, RAM total, kernel (`/proc/sys/kernel/osrelease`), distro (`/etc/os-release` `PRETTY_NAME`), hostname, discos (modelo, tamanho, SSD/HD por `queue/rotational`, NVMe/SATA), GPUs (da Etapa 18).
- [x] UI: temperatura nos cards de CPU e Disco (vermelha a partir do limiar: 10 °C abaixo do `temp*_max`, ou do `temp*_crit`, sem passar de 85 °C para CPU e 70 °C para disco; o `max` do `coretemp` é o TjMax de 100 °C, alto demais para avisar); card **Bateria** (só com bateria: nível total com barra, estado em pt-BR, potência em W, tempo restante; saúde e ciclos de cada bateria, uma linha por bateria quando houver mais de uma); card **Ficha técnica** (lista chave/valor, uptime atualizado a cada amostra).
- [x] Testes com árvores falsas de `sys/`: `coretemp` com vários núcleos, só `k10temp`, só `acpitz`, sem nenhum sensor; `cpu_thermal` (ARM); NVMe com hwmon, SATA com e sem `drivetemp`, eMMC; bateria `energy_*` e `charge_*`, carregando, descarregando, `full_design` ausente, `BAT0` + `BAT1`, mouse Bluetooth ignorado; `/etc/os-release` sem `PRETTY_NAME`.

**Pronto quando:**
- Na máquina de teste, temperaturas a ±1 °C do `sensors` e bateria igual ao `upower -i <bateria>` (nível, potência, saúde).
- Todos os formatos da "Matriz de validação" têm teste com árvore falsa e passam.
- Sem bateria o card some; sem sensor de CPU ou de disco, o campo mostra "—" sem quebrar o card.

**Resultado (23/10/2026):**

| Medida | `monitor` | Referência |
|---|---|---|
| CPU (`coretemp`, `Package id 0`) | 45 °C, limiar 85 °C | `temp1_input` do `coretemp`: 45000 (o `lm-sensors` não está instalado; ele lê o mesmo arquivo) |
| Disco (`sda`, SATA) | "—" com a dica `sudo modprobe drivetemp` | `drivetemp` não carregado |
| Bateria, energia | 23,0052 Wh de 41,9976 Wh de fábrica | `upower`: 23,0052 Wh e 41,9976 Wh |
| Bateria, saúde | 54,777% | `upower` `capacity`: 54,7774% |
| Bateria, potência (cheia, na tomada) | 0,0114 W (escondida na UI, abaixo de 0,1 W) | `upower` `energy-rate`: 0,0114 W |
| Bateria, ciclos | não informado (`cycle_count = 0`) | — |

- 18 testes novos no crate (71 no total; 156 no workspace), clippy limpo, `npm run check` sem erros. A Matriz de validação de CPU, disco e bateria está coberta por árvores falsas.
- A primeira versão calculava a potência com a tensão do momento (0,0129 W). O UPower usa a tensão nominal, a mesma da conversão de µAh para Wh. Com ela, os dois batem, e o tempo restante (Wh ÷ W) fica coerente.
- Nesta máquina, a temperatura do `dell_smm` ("CPU", 44 °C) também existe, mas o `coretemp` vem antes na ordem: é o sensor do próprio chip.
- ⏳ **Falta conferir**, o que precisa ser feito por você:
  1. Descarregando (fora da tomada): potência e tempo restante iguais ao `upower -i /org/freedesktop/UPower/devices/battery_BAT0` (`energy-rate`, `time to empty`), depois dos primeiros 10 s.
  2. Rodar `sudo modprobe drivetemp`, reabrir a aba e ver a temperatura do SSD (e a dica sumir).
  3. Conferência visual dos cards Bateria e Ficha técnica, nos temas claro e escuro.

---

## Etapa 18 — GPU
Uma abstração por fabricante, cada uma com fallback para "não exposto" (D49).
- [x] `gpu/mod.rs`: descobre as placas por `/sys/class/drm/card[0-9]*` (sem os conectores `card0-HDMI-A-1` nem os nós `renderD*`). Fabricante, driver e slot vêm do `device/uevent` (`PCI_ID`, `DRIVER`, `PCI_SLOT_NAME`), que tem o mesmo que `device/vendor` e `device/driver` num arquivo só. Ficam de fora os framebuffers do firmware (`simple-framebuffer`, `efidrm`…) e os monitores USB (`evdi`, `udl`, `gud`). **Mudança:** o nome vem do `pci.ids` lido direto, não do `lspci -mm -s <slot>`: o `lspci` lê o espaço de configuração PCI, e isso acordou a Radeon em D3cold desta máquina (`runtime_status` de `suspended` para `active`). Sem o arquivo (ou em ARM, sem PCI), a UI mostra o driver com o fabricante ("GPU AMD (amdgpu)"), e a ficha técnica, o `vendor:device` em hexadecimal. Sem nenhuma placa (VM sem GPU virtual, servidor), o card some.
- [x] `gpu/amd.rs` (`amdgpu`): `gpu_busy_percent`, `mem_info_vram_used` (o total é lido na descoberta, sem acordar) e `mem_info_gtt_*` só nas integradas (APU, reconhecida pela tensão `vddnb` do hwmon, que o `amdgpu` só cria nelas); hwmon do `device/` para `temp1_input` (`edge`), `power1_average` ou `power1_input` e `freq1_input`. Placa em D3cold (`power/runtime_status = suspended`): mostra "em repouso" **sem** ler os outros arquivos, que a acordariam (5.15) ou falham com EBUSY (kernels novos).
- [x] **Pausa para a dedicada dormir** (fora do plano original, D49): cada leitura renova o atraso de autosuspend da placa (5 s), e lendo a 1 Hz a Radeon nunca dormia. Depois de 3 leituras sem uso, a coleta da placa para por `autosuspend + 3 s`; se ela dormiu, fica "em repouso"; se continua ativa (monitor ligado nela), a pausa se desliga até ela dormir.
- [x] Consumo acima do limite da placa (`power1_cap_max`/`power1_cap` × 1,25; sem limite, 1.000 W) é descartado: a Radeon R7 M445 desta máquina, com limite de 30 W, informa 572 W e 1.088 W parada.
- [x] `gpu/nvidia.rs` (driver proprietário): um `nvidia-smi --query-gpu=pci.bus_id,utilization.gpu,memory.used,memory.total,temperature.gpu,power.draw,clocks.gr,clocks.max.gr --format=csv,noheader,nounits -lms 1000` de vida longa, lido numa thread; o `pci.bus_id` liga cada linha à placa. Só roda enquanto alguma NVIDIA está ativa e fora da pausa (com ela suspensa, não sobe). Sem `nvidia-smi` em `/usr/bin` ou `/usr/local/bin`: "não exposto", com a dica do pacote `nvidia-utils`; se ele sair sozinho, não sobe de novo na sessão. `nouveau`: só temperatura pelo hwmon. O processo morre no `Drop` (`kill` + `wait`), junto com o `Sampler`, quando a thread do monitor sai.
- [x] `gpu/intel.rs` (`i915`/`xe`): frequência atual (`gt_act_freq_mhz`, senão `gt_cur_freq_mhz`; no `xe`, `tile0/gt0/freq0/act_freq`) e máxima (`gt_max_freq_mhz`, senão `gt_RP0_freq_mhz`). Uso e consumo exigem `perf` com root: "exige root", sem tentar.
- [x] Card **GPU** (`GpuCard.svelte`): um por vez, com abas pequenas no título quando há mais de uma ("Intel · integrada", "AMD · dedicada"; começa na primeira acordada). Gráfico de uso (na Intel, da frequência em % da máxima), memória de vídeo (ou "usa a RAM"), memória compartilhada (GTT) nas APUs, frequência, temperatura e consumo; no lugar do que falta, o motivo. Some só sem nenhuma placa. A ficha técnica ganhou uma linha por GPU.
- [x] Testes: árvores falsas para `amdgpu` dedicada (ativa e suspensa) e integrada (APU, com GTT), pausa (dorme, acorda de novo, continua acordada, ocupada nunca pausa), consumo inválido, `i915` (atual e antigo, sem `act`), `xe`, `nouveau`, NVIDIA proprietária com e sem `nvidia-smi` (executável falso), NVIDIA suspensa (o `nvidia-smi` não sobe), NVIDIA ociosa (o `nvidia-smi` sai na pausa), híbrida (Intel + AMD e Intel + NVIDIA), sem placa, só framebuffer, ARM sem PCI, GPU virtual, sem `pci.ids`; parser do `pci.ids` (subsistema com nome do notebook ignorado) e da linha CSV do `nvidia-smi` (`[N/A]`, `[Not Supported]`, mais de uma GPU); o `Drop` mata o processo; e, no sistema real, que a amostra não acorda uma placa suspensa.

**Pronto quando:**
- Toda placa presente aparece com o que o driver dela expõe, e o que falta mostra o motivo ("não exposto pelo driver", "exige root", "em repouso") em vez de zero.
- Abrir a aba não acorda uma GPU dedicada suspensa (`runtime_status` continua `suspended`).
- Na máquina de teste, os valores batem com a ferramenta do fabricante (`radeontop`/`sensors` na AMD, `nvidia-smi` na NVIDIA, `intel_gpu_top` só para a frequência na Intel).
- Com NVIDIA, não sobra processo `nvidia-smi` depois de sair da aba (conferir quando houver uma máquina com NVIDIA; até lá, coberto pelo teste do `Drop` com um executável falso).

**Resultado (23/10/2026, Dell com Intel HD Graphics 620 + AMD Radeon R7 M445, kernel 7.0):**

| Medida | `monitor` | Referência |
|---|---|---|
| Placas encontradas | `card0` AMD Radeon R7 M445 (dedicada, 4,29 GB), `card1` Intel HD Graphics 620 (integrada) | `lspci -mm`: "Radeon R7 M445" (subsistema Dell) e "HD Graphics 620" |
| Radeon suspensa, 3 amostras com a aba aberta | "em repouso"; `runtime_active_time` +0 ms | antes e depois: `suspended` |
| `lspci -mm -s 01:00.0`, para comparar | — | acorda a placa (`suspended` → `active`): por isso o `pci.ids` |
| Radeon acordada, sem uso | 0%, 300 MHz, VRAM 1,39 MB de 4,29 GB, 44–48 °C | `gpu_busy_percent` 0, `freq1_input` 300 MHz, `mem_info_vram_used` 1.392.640, `temp1_input` 44000–48000 |
| Radeon acordada, depois da pausa | 8 leituras, depois "em repouso" (dormiu ~10 s depois de acordar) | sem a pausa, lendo a 1 Hz: ficou `active` os 12 s, e só dormiu ~6 s depois da última leitura |
| Consumo da Radeon | "—" (descartado) | `power1_input`: 1.088 W, 572 W e 29,8 W parada, com `power1_cap` de 30 W |
| Intel, frequência | 300–350 MHz de 1.050 MHz | `gt_act_freq_mhz` 300–350, `gt_max_freq_mhz` 1050 |
| Custo da GPU | descoberta 5,6 ms (uma vez por sessão, quase toda no `pci.ids` de 1,5 MB); 44 µs por amostra | — |

- 27 testes novos no crate (80 no total; 183 no workspace), clippy limpo, `npm run check` sem erros. Repetidos 15 vezes: sem falha e sem processo `nvidia-smi` falso sobrando. (O "71 no total" da Etapa 17 estava errado: eram 53 no crate; os 156 do workspace estavam certos.)
- As ferramentas do fabricante (`radeontop`, `sensors`, `intel_gpu_top`) não estão instaladas nesta máquina; a conferência foi contra os mesmos arquivos do sysfs que elas leem.
- ⚠️ **Achado fora desta etapa:** a amostra completa passou de ~1 ms para 20–30 ms, e não por causa da GPU. Pelo `strace`, o tempo está no `drivetemp` (agora carregado: ~8 ms por leitura, um comando SMART ao SSD) e no `BAT0/present` (6–17 ms, que vai ao EC pela ACPI) — leituras da Etapa 17. Quase tudo é espera no kernel, não CPU (o processo ficou em 0,5–1,3% de um núcleo, com o `strace`), mas o tick fica mais longo. Fica para a Etapa 20 (D51): ler a bateria e o `drivetemp` com menos frequência (a cada 5–10 s) e medir de novo.
- ⏳ **Falta conferir**, o que precisa ser feito por você:
  1. Abrir a aba Recursos com a Radeon suspensa (`cat /sys/bus/pci/devices/0000:01:00.0/power/runtime_status`): continua `suspended`, e o card mostra "em repouso" nela e a frequência na Intel. As abas do card trocam de placa.
  2. Rodar algo na Radeon (`DRI_PRIME=1 glxgears`, do pacote `mesa-utils`): o card mostra uso, temperatura e frequência subindo; ao fechar, em ~10 s ela volta a "em repouso".
  3. A linha "GPU" da ficha técnica, nos temas claro e escuro.
  4. NVIDIA, AMD integrada e `xe`: só por testes até haver uma máquina com elas.

## Etapa 19 — "Top 5" processos
- [x] `procs.rs`: a cada 2 ticks (`PROCS_EVERY`), lê `/proc/[pid]/stat` e `/proc/[pid]/io` (`read_bytes`/`write_bytes`). **Mudança:** o `stat` já traz o `comm`, o pai e o RSS, então `comm` e `statm` não são lidos (dois arquivos a menos por processo). CPU% = Δ(utime + stime) / `CLK_TCK` / Δtempo, relativo a um núcleo (como o `top`); a UI também mostra relativo ao processador inteiro. Memória pelo RSS (× tamanho de página). Processos que somem entre leituras são ignorados; o pid vem junto com o início, porque o kernel o reaproveita. Threads do kernel (`PF_KTHREAD`) e zumbis ficam de fora.
- [x] Agrupar por aplicativo (D50): threads já não aparecem (só `/proc/[pid]`); um processo entra no grupo do pai quando o pai tem o mesmo `comm`, até a raiz ("×N": Chrome, Electron, `node`). Nome pelo `.desktop` (`apps.rs`) quando o `argv[0]` da raiz é o executável do `Exec`, tem o mesmo nome ou está na pasta própria do app (o Chrome roda em `/opt/google/chrome/`, onde fica o script do `Exec`); grupos do mesmo `.desktop` se juntam. Entradas `NoDisplay` ficam de fora (o `claude` aparecia como "Claude Code URL Handler"). Sem `.desktop`, o nome do executável quando o `comm` (15 caracteres) é o começo dele, senão o `comm`.
- [x] I/O de processo só funciona para os do próprio usuário (`/proc/[pid]/io` é `0400` do dono). Os de outros usuários entram em CPU e RAM, e o critério "Disco" diz "só seus processos".
- [x] Custo: o `Procs` guarda `pid → tempos anteriores`, reaproveita o buffer e o caminho, lê cada arquivo com um `read` só, não tenta de novo o `io` que falhou e não relê o `io` de processo que não usou CPU (não fez I/O).
- [x] UI `TopProcesses.svelte`: card com seletor CPU | Memória | Disco (`MiniTabs.svelte`, que o card GPU passou a usar também), 5 linhas (nome, ×N, valor, mini-barra), com `animate:flip` na reordenação (desligado em `prefers-reduced-motion`). Critério e escala da CPU lembrados no `localStorage`. Só leitura: sem "encerrar" (fora do escopo).
- [x] CLI: `monitor --procs` imprime as três listas e, no fim, o custo da leitura de processos (média das amostras com processos menos a das sem).
- [x] Testes: árvore falsa de `proc/` com processos que somem, pid novo e pid reaproveitado, `comm` com espaço e parênteses (o `stat` exige achar o último `)`), agrupamento por pai (e `chrome` filho de outro programa fora do grupo), junção pelo `.desktop`, `comm` cortado em 15, `io` sem permissão, `io` de processo parado não relido, threads do kernel e zumbis, listas com no máximo 5; `.desktop` com aspas, `env`, snap, ações, `NoDisplay`, modos (`libreoffice --calc`) e pastas genéricas; leitura a cada 2 amostras.

**Pronto quando:**
- Rodando `stress --cpu 1` (ou `yes > /dev/null`), ele aparece no topo de CPU com ~100% em até 4 s; `dd if=/dev/zero of=~/x bs=1M count=2000` aparece no topo de Disco.
- O custo da leitura de processos fica dentro da meta, medido com o CLI (`monitor --procs`).

**Resultado (23/10/2026, 183 processos fora as threads do kernel, i7-7500U):**

| Medida | `monitor --procs` | Referência |
|---|---|---|
| `yes > /dev/null` | 2º lugar em CPU 1,7 s depois de começar (76%, intervalo parcial); 97,6% e 99,6% nas leituras seguintes | `ps`: 97,3% |
| `dd … bs=1M count=2000` (no SSD) | 1º em Disco, ↑ 904 MB/s e depois 74 MB/s | `sda` no mesmo `monitor`: 472 MB/s (o `dd` conta ao sujar o cache de páginas) |
| Agrupamento | Google Chrome ×22, Antigravity IDE ×15, `gnome-shell`, `claude` | `ps`: 18 `chrome` + 2 `chrome_crashpad_handler`… (o `/opt/google/chrome` liga os dois ao `.desktop`) |
| Custo, laço quente | 3,8 ms por leitura | meta < 5 ms com ~400 processos |
| Custo a cada 2 s (CPU da thread) | de 3,4 a 14,8 ms por leitura, quase tudo no `stat`; o `io` relido em ~20 dos 183 | — |
| Coletor inteiro a 1 Hz | **1,67%** de um núcleo (60 s; +10,2 ms por leitura de processos) | sem a leitura de processos: **1,30%** (30 s); na Etapa 15, sem processos, bateria e `drivetemp`: 0,10% |

- 17 testes novos no crate (97 no total; 200 no workspace), clippy limpo, `npm run check` sem erros.
- ⚠️ **Meta de custo só no laço quente.** Com 183 processos, a leitura cabe nos 5 ms quando a CPU está acordada; a 1 Hz, a CPU desta máquina volta a 800 MHz entre as amostras, e o mesmo trabalho custa de 3 a 15 ms de CPU. O piso é o `stat` de cada processo (o kernel soma os tempos de todas as threads); com ~400 processos, seria o dobro. Pular o `io` dos processos parados e fazer um `read` só por arquivo tiraram o resto. Opções para a Etapa 20 (D51): ler os processos a cada 3–5 s, ou só quando o card estiver na tela.
- ⚠️ **O coletor inteiro está em 1,7% de um núcleo, e o grosso não é dos processos.** Sem eles, fica em 1,30%, contra 0,10% na Etapa 15: é o custo que a Etapa 17 trouxe (o achado da Etapa 18). Cada amostra lê vários arquivos da bateria, que no Dell passam pelo EC/ACPI (o `BAT0/present` leva 6–17 ms), e o `drivetemp`, agora carregado, manda um comando SMART ao SSD (~8 ms). Os processos somam ~0,4–0,7 p.p. A meta de 0,5% do coletor (D51) fica para a Etapa 20, que deve começar por ler a bateria e o `drivetemp` a cada 5–10 s e decidir a cadência dos processos.
- O `dd` aparece com mais que a velocidade do disco porque o `write_bytes` conta ao sujar o cache de páginas; a gravação de fato, feita depois pelas threads do kernel, não aparece em nenhum processo (D50).
- ⏳ **Falta conferir**, o que precisa ser feito por você:
  1. Card Processos na aba Recursos: trocar CPU | Memória | Disco, ver as linhas animarem ao reordenar e "Ver do processador" mudar os valores.
  2. Com "Reduzir animação" ligado no GNOME (Acessibilidade), a reordenação não anima.
  3. Temas claro e escuro, e a janela no mínimo (560 px).

**Revisão (24/10/2026):**
- **Sem bateria** (D56): o card Bateria e o `battery.rs` saíram; a amostra não lê mais `/sys/class/power_supply`, e a `Sample` perdeu `batteries`/`battery_total` (no Rust e no `api.ts`). Era a leitura que passava pelo EC/ACPI (o `BAT0/present` levava 6–17 ms).
- **Ficha técnica na linha inteira:** o `MetricCard` ganhou `wide` (`grid-column: 1 / -1`), e as linhas da ficha ficam em duas colunas quando cabem (mínimo de 280 px cada); na janela estreita, uma coluna.
- Medido com `monitor --count 30`: amostra sem processos em **2,1 ms** (antes 20–30 ms); coletor inteiro a 1 Hz em **0,70%** de um núcleo (antes 1,67%), com a leitura de processos (+10,2 ms) igual. Falta a meta de 0,5% da D51, agora quase só pela leitura de processos e pelo `drivetemp` (quando carregado).
- 6 testes da bateria removidos (96 no crate; 199 no workspace), clippy limpo, `npm run check` sem erros.
- ⏳ **Falta conferir:** a ficha técnica na largura toda (duas colunas de linhas) e, na janela no mínimo (560 px), numa coluna só.

## Etapa 20 — Polimento e pacote 1.1.0
- [ ] Acessibilidade: cada gráfico com `role="img"` e `aria-label` resumindo o valor atual e o mínimo/máximo do minuto; valores com `aria-live="polite"` limitado (só o card focado), para o leitor de tela não ler tudo a cada segundo.
- [ ] `prefers-reduced-motion`: gráfico sem transição entre amostras.
- [ ] Unidades e textos revisados em pt-BR; tooltip no gráfico com o valor de cada ponto (hover/foco).
- [ ] Medição final do custo (D51) e registro no `Resultado`: CPU do processo do app com a aba Recursos aberta por 10 min, com a aba Armazenamento e minimizado.
- [ ] Docs: ARCHITECTURE §1 (diagrama), §2 (layout), nova §11 "Monitor de recursos" (fontes de cada métrica, fallbacks, ciclo de vida); DECISIONS D45–D52; README (status e capturas de tela da aba).
- [x] Versão 1.1.0 nos três manifests (`package.json`, `src-tauri/Cargo.toml`, `tauri.conf.json`) e nos crates; `npm run tauri build` gera o `.deb`.

**Pronto quando:** `cargo test --workspace`, `cargo clippy --workspace --all-targets` e `npm run check` limpos; o `.deb` instalado abre nas duas abas; conferência visual feita.

**Resultado:** _a preencher._

---

## Decisões previstas (entram no DECISIONS.md ao implementar)

| # | Decisão | Alternativas descartadas |
|---|---|---|
| D45 | Ler `/proc` e `/sys` direto num crate `monitor` próprio, com parsers puros e raiz injetável | `sysinfo`: cobre CPU/RAM/rede/processos, mas não GPU, temperatura de disco, zRAM, PSI nem saúde da bateria; teríamos dois caminhos de leitura, e ele faz mais trabalho por amostra do que precisamos. Colocar no `scanner`: misturaria dois motores sem nada em comum |
| D46 | Coleta só com a aba visível: `start`/`stop` pela UI (aba, `visibilitychange`, minimizar), e parada no backend se o `Channel` falhar | Coleta contínua com o histórico no Rust: gasta CPU e bateria com a janela fechada, que é o que a ideia proíbe. Pausar ao perder o foco: quebraria o caso de olhar o monitor durante uma tarefa em outra janela |
| D47 | Histórico de 60 amostras só no frontend; voltar à aba recomeça o gráfico | Anel no backend sempre ativo (contraria D46). Guardar no cache em disco: sem uso para dados de 1 min |
| D48 | Gráficos em SVG nativo com Svelte, sem biblioteca | `uPlot` (Canvas, ~45 kB): mais rápido com milhares de pontos, mas aqui são ≤ 8 séries × 60 pontos, e o SVG herda os tokens do tema e a acessibilidade de graça |
| D49 | GPU por fabricante com fallback explícito; `nvidia-smi` de vida longa; nunca acordar GPU suspensa (nome pelo `pci.ids`, pausa para a dedicada ociosa dormir) | NVML via `nvml-wrapper`: carrega `libnvidia-ml.so` em tempo de execução, mais rápido, mas adiciona dependência e falha de forma menos previsível sem o driver (avaliar se o custo do `nvidia-smi` passar da meta). `intel_gpu_top`: exige root |
| D50 | "Top 5" agrupado por aplicativo (mesmo `comm` do pai, depois o `.desktop`), só leitura | Lista por PID: o Chrome ocuparia as 5 linhas. Botão "encerrar": ação destrutiva fora do propósito do app |
| D51 | Metas de custo: < 0,5% de um núcleo no coletor, < 2% no app inteiro com a aba aberta, 0 com a aba fechada | — |
| D52 | Mínimo Ubuntu 22.04 (kernel 5.15), `amd64`; nenhum critério preso a um hardware; ferramentas externas opcionais | Suportar 20.04 (kernel 5.4): sem hwmon de NVMe nem `gpu_busy_percent` em parte das placas, e a 20.04 já saiu do suporte padrão. Exigir `lspci`/`nvidia-smi`: quebraria em instalações mínimas |

## Matriz de validação

Cada formato tem um teste com árvore falsa de `proc/`/`sys/` (montada a partir de saídas reais, coletadas de máquinas ou de relatos públicos). A coluna "Real" marca o que já foi conferido num computador de verdade e entra no "Resultado" de cada etapa.

| Área | Formatos cobertos por teste | Real |
|---|---|---|
| CPU | Intel (`coretemp`), AMD (`k10temp`, `zenpower`), ARM (`cpu_thermal`), sem sensor, sem `cpufreq` (VM), de 2 a 64 threads, núcleo desligado | Intel i7-7500U (`coretemp`), 4 threads |
| Memória | Com swap em arquivo/partição, sem swap, zRAM, kernel sem PSI | _a preencher_ |
| Disco | SATA (com/sem `drivetemp`), NVMe, eMMC/SD, USB, virtio, `md` e `dm` ignorados | SATA sem `drivetemp` |
| Rede | Cabo, Wi-Fi, os dois juntos, interfaces virtuais e VPN ignoradas, contador reiniciado | _a preencher_ |
| GPU | AMD dedicada/integrada/suspensa, NVIDIA com/sem `nvidia-smi` e suspensa, `nouveau`, Intel `i915`/`xe`, híbrida, sem GPU, só framebuffer, ARM, virtual, sem `pci.ids` | Intel `i915` (HD 620) + AMD `amdgpu` dedicada (R7 M445, suspensa e ativa) |
| Sessão | X11 e Wayland (pausa ao minimizar), 1366×768, HiDPI 200% | _a preencher_ |

## Riscos e pontos a validar

- **Minimizar no WebKitGTK:** o `visibilitychange` pode não disparar ao minimizar; por isso o `onResized` + `isMinimized()`. Validar no GNOME (X11 e Wayland) na Etapa 16; no Wayland, `isMinimized()` pode não ser confiável — nesse caso, aceitar e documentar.
- **Custo do WebKit:** redesenhar 8 SVGs por segundo deve ser barato, mas medir na Etapa 16; se passar da meta, desenhar só os cards visíveis (`IntersectionObserver`).
- **Contadores de 32 bits** no `net/dev` de alguns drivers dão a volta; tratar diferença negativa como reinício (sem pico falso no gráfico).
- **Máquinas variadas:** o desenvolvimento tem acesso a poucas combinações de hardware. Os testes com árvores falsas cobrem os formatos da "Matriz de validação"; o que ainda não foi visto num computador real fica marcado como pendente, e um relato de usuário com a saída do `monitor --json` vira uma nova árvore de teste.
- **Live USB e VMs:** sem sensores, sem `cpufreq` e às vezes sem GPU. É o caso "quase tudo `None`": a aba precisa continuar útil só com CPU, memória, disco e rede.
