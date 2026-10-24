# PLAN V2 — Aba "Recursos"

Plano de implementação da ideia em [ideia-recursos.md](ideia-recursos.md): uma segunda aba, **Recursos**, com a telemetria do computador em tempo real (CPU, memória, disco, GPU, rede, processos, bateria e ficha técnica), ao lado da aba **Armazenamento**, que já existe.

A numeração continua a do [PLAN.md](PLAN.md) (Etapas 15 a 20), para manter os commits no padrão `feat: etapa N - <descrição>`. As decisões previstas abaixo (D45 a D52) entram no [DECISIONS.md](DECISIONS.md) quando forem implementadas, com o resultado medido. O "como funciona" vai para o [ARCHITECTURE.md](ARCHITECTURE.md) (nova §11) e o progresso para a tabela de status do README.

**Escopo da V2:**
- Seletor centralizado no topo: **Armazenamento | Recursos**.
- Cards com gráfico dos últimos 60 s: CPU (total, por núcleo, frequência, temperatura), Memória (RAM, swap/zRAM, pressão), Disco (leitura/gravação em MB/s, temperatura), GPU (uso, VRAM, temperatura, consumo) e Rede (download/upload e total da sessão).
- "Top 5" processos por CPU, RAM ou disco.
- Bateria (só em notebook) e ficha técnica.
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
│  cpu.rs · mem.rs · disk_io.rs · net.rs · sensors.rs · gpu/ · battery.rs · procs.rs    │
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
    batteries: Vec<BatterySample>,    // uma por BAT*: name, pct, status, energy_now/full, power_w, health_pct, cycles
    battery_total: Option<BatteryTotal>, // soma das baterias: pct ponderado pela energia, power_w, time_left_s
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
- [x] `cpu.rs`: `/proc/stat` (linha `cpu` e `cpuN`), uso = 1 − Δ(idle + iowait) / Δtotal. Frequência por `/sys/devices/system/cpu/cpu*/cpufreq/scaling_cur_freq` (média e máxima), com `/proc/cpuinfo` (`cpu MHz`) como alternativa.
- [x] `mem.rs`: `/proc/meminfo`. Usada = `MemTotal − MemAvailable` (como o `free` e o GNOME). Swap de `SwapTotal/SwapFree`; zRAM de `/sys/block/zram*/mm_stat` (dados originais e comprimidos, para mostrar a taxa de compressão). Pressão de `/proc/pressure/memory` (`some avg10`), quando o kernel tiver PSI.
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

---

## Etapa 16 — Ponte Tauri e aba "Recursos"
Primeira versão visível: o seletor de abas e os cards da Etapa 15 com gráficos.
- [ ] `src-tauri/src/state/monitor.rs`: `MonitorState { running: AtomicBool, generation: AtomicU64 }`, separado do `AppState` do scan (não divide lock com ele). `start(interval, sampler_factory, send)` sobe a thread; um novo `start` incrementa a `generation`, e a thread antiga para no próximo tick. A thread para sozinha se o `send` falhar (janela fechada ou recarregada).
- [ ] Commands `start_monitor(on_event: Channel<MonitorEvent>)`, `stop_monitor()` e `system_info()` — os 4 passos do CLAUDE.md (`commands.rs`, `lib.rs`, `build.rs`, `capabilities/default.json`) + wrappers em `api.ts` com os tipos espelhados.
- [ ] O monitor **não** é recusado durante um scan: é justamente quando ver o disco trabalhando interessa. A coleta não toca no disco escaneado (só `/proc` e `/sys`, que são memória).
- [ ] **Pausa** (D46): a UI chama `start_monitor` ao mostrar a aba e `stop_monitor` ao sair dela, em `visibilitychange` (`hidden`) e ao minimizar (`onResized` + `isMinimized()`; conferir se o `core:default` já inclui `core:window:allow-is-minimized`). Perder o foco **não** pausa: o caso de uso é olhar o monitor enquanto outra janela faz a tarefa pesada.
- [ ] `TabSwitch.svelte`: controle segmentado no topo, centralizado, estilo iOS (`role="tablist"`, setas do teclado). A aba Armazenamento fica escondida, não desmontada (a barra não refaz a animação, como na volta do detalhe). A aba escolhida é lembrada em `localStorage` (com `try/catch`), mas o app sempre abre escaneando.
- [ ] `ResourcesView.svelte`: grade responsiva de cards (`grid-template-columns: repeat(auto-fill, minmax(…))`: 1 coluna na janela estreita, 2 ou 3 nas largas). Mini-barras por núcleo quebram em linhas, para caberem de 2 a 64 threads. SVG com `viewBox` e `vector-effect: non-scaling-stroke`, nítido em HiDPI. `MetricCard.svelte` (título, valor grande, subtítulo, gráfico) e `Sparkline.svelte`: SVG nativo, `<polyline>`/`<path>` com área preenchida, escala Y fixa para porcentagens e automática (com teto arredondado) para taxas; duas séries para leitura/gravação e download/upload (D48).
- [ ] Cards desta etapa: **CPU** (total + mini-barras por núcleo + frequência), **Memória** (RAM com barra usada/total e swap/zRAM; selo "sob pressão" com PSI `avg10` > 10%), **Disco** (leitura/gravação por disco), **Rede** (download/upload + total da sessão).
- [ ] `format.ts`: `formatRate` (B/s → kB/s → MB/s, decimal como o resto), `formatPercent` já existe, `formatMHz`, `formatTemp`, `formatDuration` (uptime e tempo de bateria).
- [ ] `theme.css`: tokens `--res-cpu`, `--res-mem`, `--res-disk-read`, `--res-disk-write`, `--res-net-rx`, `--res-net-tx`, `--res-gpu`, nos dois temas.
- [ ] Testes do `MonitorState` com `mpsc` e `Sampler` falso (como D24): ciclo `sample`* → `stopped`, `start` duplo (só a geração nova emite), parada quando o `send` falha.

**Pronto quando:**
- Trocar de aba não perde o estado do armazenamento (detalhe aberto, ordenação, seleção).
- Com a aba Armazenamento visível ou a janela minimizada, `top -p <pid>` mostra o app parado (sem ticks). Com a aba Recursos visível, o processo do app (Rust + WebKit) fica abaixo de 2% de um núcleo.
- Os números dos cards batem com o CLI da Etapa 15.
- A aba funciona em 1366×768 sem rolagem horizontal, e em HiDPI (escala 200%) os gráficos ficam nítidos.
- A pausa ao minimizar foi conferida em X11 e em Wayland (GNOME); a limitação, se houver, está documentada.

**Resultado:** _a preencher._

---

## Etapa 17 — Sensores, bateria e ficha técnica
Tudo que depende de `/sys/class/hwmon`, `/sys/class/power_supply` e varia de máquina para máquina.
- [ ] `sensors.rs`: varre `/sys/class/hwmon/hwmon*` uma vez (no `Sampler::new`) e guarda os arquivos de cada sensor; depois só lê `temp*_input`. Ordem de preferência para a **CPU**: `coretemp` (`Package id 0`), `k10temp` (`Tctl`/`Tdie`), `zenpower`, `cpu_thermal` (ARM); sem nenhum, a `thermal_zone` do tipo `x86_pkg_temp`. `acpitz` só como último recurso, marcado como aproximado.
- [ ] Temperatura do **disco**: `nvme` (hwmon ligado a `/sys/block/nvmeXnY/device`) e `drivetemp` (SATA, liga o hwmon ao disco pelo `device`). SATA sem o módulo: "—" com a dica `sudo modprobe drivetemp` (o app só mostra o comando, D44). eMMC, cartão SD, USB e disco de VM: "—" sem dica (não há sensor).
- [ ] `battery.rs`: **todas** as `/sys/class/power_supply/*` com `type = Battery` e `scope` diferente de `Device` (fora mouse e fone Bluetooth). Aceita os nomes `BAT*`, `CMB*` e outros: o filtro é o `type`, não o nome. Aceita os dois formatos: `energy_*` (µWh) + `power_now`, ou `charge_*` (µAh) + `current_now` × `voltage_now` (convertido para µWh com `voltage_min_design`, ou `voltage_now` sem ele). Saúde = `full / full_design`; tempo restante = energia restante / potência (descarregando) ou faltante / potência (carregando), escondido com potência ≈ 0 ou nos primeiros 10 s (valor instável). Ciclos de `cycle_count` quando ≠ 0. Com mais de uma bateria (ThinkPad com interna + externa), `battery_total` soma energia e potência, e o nível é ponderado pela energia de cada uma, como o UPower.
- [ ] `system_info()`: modelo da CPU (`/proc/cpuinfo` `model name`, ou `/sys/firmware/devicetree/base/model`), núcleos físicos e threads, RAM total, kernel (`uname`), distro (`/etc/os-release` `PRETTY_NAME`), hostname, discos (modelo, tamanho, SSD/HD por `queue/rotational`, NVMe/SATA), GPUs (da Etapa 18).
- [ ] UI: temperatura nos cards de CPU e Disco (cor muda acima de um limiar do próprio sensor: `temp*_crit` ou `temp*_max` quando existirem, senão 85 °C para CPU e 70 °C para disco); card **Bateria** (só com bateria: nível total com barra, estado em pt-BR, potência em W, tempo restante; saúde e ciclos de cada bateria, uma linha por bateria quando houver mais de uma); card **Ficha técnica** (lista chave/valor, uptime atualizado a cada amostra).
- [ ] Testes com árvores falsas de `sys/`: `coretemp` com vários núcleos, só `k10temp`, só `acpitz`, sem nenhum sensor; `cpu_thermal` (ARM); NVMe com hwmon, SATA com e sem `drivetemp`, eMMC; bateria `energy_*` e `charge_*`, carregando, descarregando, `full_design` ausente, `BAT0` + `BAT1`, mouse Bluetooth ignorado; `/etc/os-release` sem `PRETTY_NAME`.

**Pronto quando:**
- Na máquina de teste, temperaturas a ±1 °C do `sensors` e bateria igual ao `upower -i <bateria>` (nível, potência, saúde).
- Todos os formatos da "Matriz de validação" têm teste com árvore falsa e passam.
- Sem bateria o card some; sem sensor de CPU ou de disco, o campo mostra "—" sem quebrar o card.

**Resultado:** _a preencher._

---

## Etapa 18 — GPU
Uma abstração por fabricante, cada uma com fallback para "não exposto" (D49).
- [ ] `gpu/mod.rs`: descobre as placas por `/sys/class/drm/card[0-9]*` (sem os conectores `card0-HDMI-A-1`), fabricante por `device/vendor` (`0x1002` AMD, `0x10de` NVIDIA, `0x8086` Intel) e driver por `device/driver`. Nome pelo `lspci -mm -s <slot>` uma vez; sem `pciutils` (ou em ARM, sem PCI), o nome do driver com o fabricante ("GPU AMD (amdgpu)"), e `vendor:device` em hexadecimal na ficha técnica. Sem nenhuma placa em `/sys/class/drm` (VM sem GPU virtual, servidor), o card some.
- [ ] `gpu/amd.rs` (`amdgpu`): `gpu_busy_percent`, `mem_info_vram_used/total` (e `mem_info_gtt_*` para integradas), hwmon do `device/` para `temp1_input`, `power1_average` ou `power1_input` e `freq1_input`. Placa em D3cold (`power/runtime_status = suspended`, comum em notebook com GPU dedicada parada): mostra "em repouso" **sem** ler os outros arquivos, que a acordariam.
- [ ] `gpu/nvidia.rs` (driver proprietário): um `nvidia-smi --query-gpu=utilization.gpu,memory.used,memory.total,temperature.gpu,power.draw,clocks.gr --format=csv,noheader,nounits -lms 1000` de vida longa enquanto o monitor roda (iniciar um processo por segundo custa caro), lido numa thread (como o `run_with_timeout` do Docker). Sem `nvidia-smi`: "métricas não expostas pelo driver". `nouveau`: só temperatura pelo hwmon. O processo morre junto com o `stop_monitor` (`Drop` com `kill` + `wait`).
- [ ] `gpu/intel.rs` (`i915`/`xe`): frequência atual (`gt_act_freq_mhz` / `gt_cur_freq_mhz`) e máxima. Uso e memória exigem `perf` com root: "uso não exposto sem root", sem tentar.
- [ ] Card **GPU**: um por placa (abas pequenas quando houver mais de uma, como integrada + dedicada), com o que ela tiver; some só se não houver nenhuma placa.
- [ ] Testes: árvores falsas para `amdgpu` dedicada (ativa e suspensa) e integrada (APU, só GTT), `i915`, `xe`, `nouveau`, NVIDIA proprietária com e sem `nvidia-smi`, híbrida (integrada + dedicada), sem placa, conectores ignorados, sem `lspci`; parser da linha CSV do `nvidia-smi` (incluindo `[N/A]`, `[Not Supported]` e mais de uma GPU).

**Pronto quando:**
- Toda placa presente aparece com o que o driver dela expõe, e o que falta mostra o motivo ("não exposto pelo driver", "exige root", "em repouso") em vez de zero.
- Abrir a aba não acorda uma GPU dedicada suspensa (`runtime_status` continua `suspended`).
- Na máquina de teste, os valores batem com a ferramenta do fabricante (`radeontop`/`sensors` na AMD, `nvidia-smi` na NVIDIA, `intel_gpu_top` só para a frequência na Intel).
- Com NVIDIA, não sobra processo `nvidia-smi` depois de sair da aba (conferir quando houver uma máquina com NVIDIA; até lá, coberto pelo teste do `Drop` com um executável falso).

**Resultado:** _a preencher._

---

## Etapa 19 — "Top 5" processos
- [ ] `procs.rs`: a cada 2 ticks, lê `/proc/[pid]/stat` (utime + stime, RSS), `/proc/[pid]/comm` e `/proc/[pid]/io` (`read_bytes`/`write_bytes`). CPU% = Δ(utime + stime) / Δtempo / `CLK_TCK`, relativo a um núcleo (como o `top`), com a opção de mostrar relativo ao total. Memória pelo RSS (`statm` × tamanho de página). Processos que somem entre leituras são ignorados sem erro.
- [ ] Agrupar por aplicativo (D50): threads já não aparecem (só `/proc/[pid]`); processos com o mesmo `comm` e mesmo pai que também tenha esse `comm` somam numa linha com "×N" (Chrome, Electron, `node`). Nome amigável pelo `.desktop` quando o `cmdline` aponta para um executável conhecido; senão o `comm`.
- [ ] I/O de processo só funciona para os do próprio usuário (`/proc/[pid]/io` é `0400` do dono). Os de outros usuários entram em CPU e RAM, e o critério "Disco" diz "só seus processos".
- [ ] Custo: o `Sampler` guarda o mapa `pid → tempos anteriores` e reaproveita os buffers. Meta: < 5 ms por leitura com ~400 processos.
- [ ] UI `TopProcesses.svelte`: card com seletor CPU | Memória | Disco, 5 linhas (nome, ×N, valor, mini-barra), com `animate:flip` na reordenação (desligado em `prefers-reduced-motion`). Só leitura: sem "encerrar" (fora do escopo).
- [ ] Testes: árvore falsa de `proc/` com processos que somem, `comm` com espaço e parênteses (o `stat` exige achar o último `)`), agrupamento por pai, `io` sem permissão.

**Pronto quando:**
- Rodando `stress --cpu 1` (ou `yes > /dev/null`), ele aparece no topo de CPU com ~100% em até 4 s; `dd if=/dev/zero of=~/x bs=1M count=2000` aparece no topo de Disco.
- O custo da leitura de processos fica dentro da meta, medido com o CLI (`monitor --procs`).

**Resultado:** _a preencher._

---

## Etapa 20 — Polimento e pacote 1.1.0
- [ ] Acessibilidade: cada gráfico com `role="img"` e `aria-label` resumindo o valor atual e o mínimo/máximo do minuto; valores com `aria-live="polite"` limitado (só o card focado), para o leitor de tela não ler tudo a cada segundo.
- [ ] `prefers-reduced-motion`: gráfico sem transição entre amostras.
- [ ] Unidades e textos revisados em pt-BR; tooltip no gráfico com o valor de cada ponto (hover/foco).
- [ ] Medição final do custo (D51) e registro no `Resultado`: CPU do processo do app com a aba Recursos aberta por 10 min, com a aba Armazenamento e minimizado.
- [ ] Docs: ARCHITECTURE §1 (diagrama), §2 (layout), nova §11 "Monitor de recursos" (fontes de cada métrica, fallbacks, ciclo de vida); DECISIONS D45–D52; README (status e capturas de tela da aba).
- [ ] Versão 1.1.0 nos três manifests (`package.json`, `src-tauri/Cargo.toml`, `tauri.conf.json`) e nos crates; `npm run tauri build` gera o `.deb`.

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
| D49 | GPU por fabricante com fallback explícito; `nvidia-smi` de vida longa; nunca acordar GPU suspensa | NVML via `nvml-wrapper`: carrega `libnvidia-ml.so` em tempo de execução, mais rápido, mas adiciona dependência e falha de forma menos previsível sem o driver (avaliar se o custo do `nvidia-smi` passar da meta). `intel_gpu_top`: exige root |
| D50 | "Top 5" agrupado por aplicativo, só leitura | Lista por PID: o Chrome ocuparia as 5 linhas. Botão "encerrar": ação destrutiva fora do propósito do app |
| D51 | Metas de custo: < 0,5% de um núcleo no coletor, < 2% no app inteiro com a aba aberta, 0 com a aba fechada | — |
| D52 | Mínimo Ubuntu 22.04 (kernel 5.15), `amd64`; nenhum critério preso a um hardware; ferramentas externas opcionais | Suportar 20.04 (kernel 5.4): sem hwmon de NVMe nem `gpu_busy_percent` em parte das placas, e a 20.04 já saiu do suporte padrão. Exigir `lspci`/`nvidia-smi`: quebraria em instalações mínimas |

## Matriz de validação

Cada formato tem um teste com árvore falsa de `proc/`/`sys/` (montada a partir de saídas reais, coletadas de máquinas ou de relatos públicos). A coluna "Real" marca o que já foi conferido num computador de verdade e entra no "Resultado" de cada etapa.

| Área | Formatos cobertos por teste | Real |
|---|---|---|
| CPU | Intel (`coretemp`), AMD (`k10temp`, `zenpower`), ARM (`cpu_thermal`), sem sensor, sem `cpufreq` (VM), de 2 a 64 threads, núcleo desligado | _a preencher_ |
| Memória | Com swap em arquivo/partição, sem swap, zRAM, kernel sem PSI | _a preencher_ |
| Disco | SATA (com/sem `drivetemp`), NVMe, eMMC/SD, USB, virtio, `md` e `dm` ignorados | _a preencher_ |
| Rede | Cabo, Wi-Fi, os dois juntos, interfaces virtuais e VPN ignoradas, contador reiniciado | _a preencher_ |
| GPU | AMD dedicada/integrada/suspensa, NVIDIA com/sem `nvidia-smi`, `nouveau`, Intel `i915`/`xe`, híbrida, sem GPU, sem `lspci` | _a preencher_ |
| Bateria | `energy_*`, `charge_*`, duas baterias, sem `full_design`, periférico Bluetooth, desktop sem bateria | _a preencher_ |
| Sessão | X11 e Wayland (pausa ao minimizar), 1366×768, HiDPI 200% | _a preencher_ |

## Riscos e pontos a validar

- **Minimizar no WebKitGTK:** o `visibilitychange` pode não disparar ao minimizar; por isso o `onResized` + `isMinimized()`. Validar no GNOME (X11 e Wayland) na Etapa 16; no Wayland, `isMinimized()` pode não ser confiável — nesse caso, aceitar e documentar.
- **Custo do WebKit:** redesenhar 8 SVGs por segundo deve ser barato, mas medir na Etapa 16; se passar da meta, desenhar só os cards visíveis (`IntersectionObserver`).
- **Contadores de 32 bits** no `net/dev` de alguns drivers dão a volta; tratar diferença negativa como reinício (sem pico falso no gráfico).
- **Máquinas variadas:** o desenvolvimento tem acesso a poucas combinações de hardware. Os testes com árvores falsas cobrem os formatos da "Matriz de validação"; o que ainda não foi visto num computador real fica marcado como pendente, e um relato de usuário com a saída do `monitor --json` vira uma nova árvore de teste.
- **Live USB e VMs:** sem sensores, sem `cpufreq` e às vezes sem GPU. É o caso "quase tudo `None`": a aba precisa continuar útil só com CPU, memória, disco e rede.
