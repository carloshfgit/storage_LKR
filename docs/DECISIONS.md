# DECISIONS — Registro de decisões

Cada decisão tem o que foi decidido, o porquê e o que foi descartado. Quando há números, eles foram medidos nesta máquina: Ubuntu 24.04, SSD de 234 GB, 4 núcleos, cerca de 1,07 milhão de arquivos em `/`.

**Status:** ✅ implementada · 📋 decidida, ainda não implementada · ⚠️ limitação conhecida

## Índice

| # | Decisão | Status | Etapa |
|---|---|---|---|
| [D01](#d01) | Rust no motor | ✅ | 0–1 |
| [D02](#d02) | Tauri v2 + Svelte 5 + CSS puro na interface | ✅ | 0 |
| [D03](#d03) | Motor num crate separado, sem Tauri | ✅ | 1 |
| [D04](#d04) | 7 segmentos na barra, com "Outros" | ✅ | 1–2 |
| [D05](#d05) | Barra sempre soma 100% do disco | ✅ | 1 |
| [D06](#d06) | Medir espaço alocado, não tamanho aparente | ✅ | 1 |
| [D07](#d07) | Não cruzar filesystems nem seguir symlinks | ✅ | 1 |
| [D08](#d08) | Contar hardlinks uma vez e contar diretórios | ✅ | 1 |
| [D09](#d09) | Escanear `/` sem root | ✅ | 1 |
| [D10](#d10) | Só os 200 maiores arquivos por categoria vão para a UI | ✅ | 1 |
| [D11](#d11) | 8 threads (2× núcleos) | ✅ | 1 |
| [D12](#d12) | Classificar primeiro pelo caminho, depois pela extensão | ✅ | 2 |
| [D13](#d13) | Sem leitura de conteúdo (magic bytes) | ✅ | 2 |
| [D14](#d14) | Lixeira contada à parte | ✅ | 2 |
| [D15](#d15) | Docker via `docker system df` | ✅ | 2 |
| [D16](#d16) | Docker não bloqueia o scan | ✅ | 2–3 |
| [D17](#d17) | Limpeza: Lixeira sim, `rm` nunca | 📋 | 6 |
| [D18](#d18) | Limpeza do Docker sem volumes | 📋 | 6 |
| [D19](#d19) | Validação de segurança no Rust, não na UI | ✅ permissões · 📋 ações | 3–6 |
| [D20](#d20) | Unidades decimais (GB = 10⁹) | ✅ | 1–4 |
| [D21](#d21) | Regras de apps portáteis e de build/dev | ✅ | 2 |
| [D22](#d22) | `.ts` não é vídeo | ✅ | 2 |
| [D23](#d23) | Mídia dentro de projetos de código conta como mídia | ⚠️ | — |
| [D24](#d24) | Ciclo do scan num serviço testável, com descarte por geração | ✅ | 3 |
| [D25](#d25) | Permissão explícita por command (ACL do Tauri) | ✅ | 3 |
| [D26](#d26) | Caminhos não UTF-8 serializados com perdas | ✅ | 3 |
| [D27](#d27) | Motor otimizado também no `tauri dev` | ✅ | 3 |
| [D28](#d28) | SvelteKit em modo SPA no lugar de Svelte puro | ✅ | 0–3 |
| [D29](#d29) | Barra ao vivo com faixa "ainda não contado" | ✅ | 4 |
| [D30](#d30) | Reescanear zera a barra; cancelar volta ao resultado anterior | ✅ | 4 |
| [D31](#d31) | "Abrir na pasta" como command próprio, validado no Rust | ✅ | 5 |
| [D32](#d32) | Docker com tamanho e liberável por tipo | ✅ | 5 |

---

## Stack

### D01 — Rust no motor <a id="d01"></a>
**Decisão:** o scan, a classificação e a limpeza são escritos em Rust.

**Por quê:**
- O walk paralelo (`jwalk`) usa todos os núcleos, sem GIL.
- Há acesso direto a `st_blocks`, `st_dev`, `st_ino` e `statvfs`.
- O crate `trash` implementa a especificação da Lixeira do FreeDesktop.
- Medido: `/` com cache quente em cerca de 2,2 s.

**Descartado:** Python, de 3 a 5 vezes mais lento num walk de 1 milhão de entradas.

**Custo aceito:** curva de aprendizado e um primeiro build lento.

### D02 — Tauri v2 + Svelte 5 + CSS puro <a id="d02"></a>
**Decisão:** a interface é web, renderizada pelo WebKitGTK do sistema dentro do Tauri.

**Por quê:**
- A barra segmentada estilo iOS é só `display:flex` + `transition` em CSS.
- O Tauri ocupa cerca de 30 a 50 MB de RAM, gera o `.deb` e tem backend nativo em Rust.
- O Svelte tem animação embutida (`tweened`) e bundle mínimo.

**Descartados:**

| Alternativa | Motivo |
|---|---|
| GTK4/libadwaita | A barra e as animações custom exigiriam Cairo |
| Electron | Mais de 150 MB de RAM |
| egui, iced, Slint | Visual de ferramenta, estilização manual |
| Tailwind / lib de componentes | Peso sem ganho para cerca de 5 componentes |

### D03 — Motor num crate separado <a id="d03"></a>
**Decisão:** `crates/scanner` não depende do Tauri. Tem um CLI próprio (`scan`).

**Por quê:** dá para testar com `cargo test` e medir contra `du`/`df` sem abrir a janela. Toda a validação das Etapas 1 e 2 foi feita assim.

---

## Modelo da barra

### D04 — 7 segmentos, com "Outros" <a id="d04"></a>
**Decisão:** Fotos, Vídeos, Áudio, Documentos, Apps/Sistema, **Outros** e Livre. Escolha sua, no planejamento.

**Por quê:** caches, código, `.zip`/`.iso` e configs não são mídia nem sistema. Jogá-los em Apps/Sistema esconderia lixo do seu `$HOME` que pode ser apagado. iOS e Android também têm "Outros".

### D05 — Barra sempre soma 100% do disco <a id="d05"></a>
**Decisão:** os números vêm do `statvfs`:
- `Livre = f_bavail` (o que o usuário pode usar de fato)
- `Usado = total − Livre`
- `restante = Usado − escaneado − Docker` → entra em Apps/Sistema como "reservado/sem acesso"

**Por quê:** sem root, parte do disco é ilegível, e o ext4 reserva cerca de 5%. Se a barra mostrasse só o escaneado, nunca bateria com o "Sobre" do sistema. É o mesmo truque do "Sistema" do iOS.

**Medido:** restante de 9,15 GB, praticamente só os blocos reservados.

### D20 — Unidades decimais <a id="d20"></a>
**Decisão:** 1 GB = 10⁹ bytes.

**Por quê:** é o que o GNOME e o iOS mostram. Com GiB, os números não bateriam com o "Sobre" do Ubuntu.

**Na UI (`format.ts`):** 3 algarismos significativos com casas fixas ("5,77 GB", "12,4 GB", "155 GB"). Casas fixas evitam que os números mudem de largura a cada `progress`.

---

## Motor de scan

### D06 — Espaço alocado, não tamanho aparente <a id="d06"></a>
**Decisão:** o tamanho de cada arquivo é `st_blocks × 512`.

**Por quê:** é o que o `df` e o `du` contam. Arquivos esparsos e milhares de arquivos pequenos (cada um ocupa no mínimo 4 KB) distorceriam o total.

**Medido:** `$HOME` bate **byte a byte** com `du -sxB1 ~`.

### D07 — Não cruzar filesystems nem seguir symlinks <a id="d07"></a>
**Decisão:** só desce em diretórios com o mesmo `st_dev` da raiz. Symlinks não são seguidos.

**Por quê:** `/proc`, `/sys`, `/run`, os squashfs de `/snap/*` e pendrives não são o disco principal. Seguir symlinks contaria coisas em dobro ou entraria em loop.

### D08 — Hardlinks uma vez, diretórios também contam <a id="d08"></a>
**Decisão:**
- Arquivos com `nlink > 1` só contam na primeira vez que o inode aparece.
- Os blocos dos próprios diretórios são somados.

**Por quê:** é como o `du` faz. Sem isso, o total não fecharia com o `df`.

O inode sozinho basta como chave porque D07 garante um único filesystem.

### D09 — Escanear `/` sem root <a id="d09"></a>
**Decisão:** o app roda como usuário comum. Os 75 diretórios sem permissão são contados e ignorados.

**Por quê:** pedir senha a cada abertura é atrito e risco. O que não dá para ler cai no restante (D05), e a maior parte dele é identificada pelo Docker (D15).

**Futuro:** um modo `pkexec` opcional, no backlog.

### D10 — Só os 200 maiores por categoria vão para a UI <a id="d10"></a>
**Decisão:** um min-heap de 200 itens por categoria. O caminho completo só é montado quando o arquivo entra no heap.

**Por quê:** enviar 1 milhão de arquivos para o WebKit travaria a interface. Para decidir o que apagar, só os maiores importam.

### D11 — 8 threads <a id="d11"></a>
**Decisão:** o padrão é `2 × núcleos`.

**Por quê:** `stat` é limitado por I/O, e threads extras mantêm a fila do SSD cheia.

**Medido:**

| Threads | Tempo |
|---|---|
| 4 | 2,24 s |
| **8** | **2,11 s** |
| 16 | 2,27 s |

---

## Classificação

### D12 — Caminho primeiro, extensão depois <a id="d12"></a>
**Decisão:** 8 regras em ordem; a primeira que casar vence (ARCHITECTURE §5).
1. Pastas de sistema e de apps gerenciados → Apps/Sistema
2. Lixeira → Outros
3. Apps portáteis → Apps/Sistema (D21)
4. Qualquer coisa oculta dentro do `$HOME` → Outros
5. Build/dev → Outros (D21)
6. e 7. Arquivo visível no `$HOME` → decide pela extensão
8. Fora do `$HOME` → Apps/Sistema

**Por quê:** só pela extensão, um `.png` de ícone em `/usr/share` e os JPG do cache do Chrome virariam "Fotos".

**Medido:** Fotos caiu de 1,35 GB para 0,31 GB quando as regras entraram.

**Detalhes:**
- Implementado como `Classifier::new(home)`, que monta uma vez os caminhos do `$HOME`.
- Os prefixos são comparados por bytes e respeitam componentes inteiros (`/usrdata` não é `/usr`, e `~/snapshots` não é `~/snap`).
- Não custou desempenho: o walk continua em 2,0 a 2,5 s.

### D13 — Sem leitura de conteúdo <a id="d13"></a>
**Decisão:** a classificação usa só caminho e extensão; nenhum arquivo é aberto.

**Por quê:** ler os magic bytes custaria `open` + `read` em 1 milhão de arquivos e mataria o requisito de velocidade. Arquivos sem extensão vão para Outros.

### D21 — Regras de apps portáteis e de build/dev <a id="d21"></a>
**Problema (era uma limitação):** o Antigravity IDE descompactado em `~/Área de trabalho/` e as pastas `node_modules` em lugares visíveis tinham seus `.png`, `.mp3` e `.md` contados como Fotos, Áudio e Documentos.

**Decisão:** duas regras novas, sem I/O e decididas na mesma passada pelos componentes do caminho:

| Regra | Casa quando | Vai para |
|---|---|---|
| **App portátil** (3) | Arquivo `*.AppImage`; `~/Applications`, `~/Apps`, `~/Aplicativos`, `~/bin`; ou `resources/app`, `resources/app.asar`, `resources/app.asar.unpacked` (Electron) | Apps/Sistema |
| **Build/dev** (5) | `node_modules`, `__pycache__`, `site-packages`; ou `target/debug` / `target/release` | Outros |

**Por quê:**
- Cada padrão é uma convenção forte: AppImageLauncher usa `~/Applications`, Electron usa `resources/app`, Cargo usa `target/debug|release`.
- Casar componentes inteiros mantém o custo de I/O em zero.

**Ajustes feitos sobre a proposta inicial:**
- **App portátil vem antes de build/dev e de ocultos.** Apps Electron trazem o próprio `node_modules`; o `demo.gif` do Antigravity está em `resources/app/node_modules/…`.
- **`target` só com `debug`/`release`** e **sem `vendor`:** os dois são nomes comuns (`~/Documentos/vendor/nota.pdf` continua Documento).
- **Lixeira antes de app portátil:** um AppImage na Lixeira é Lixeira.
- **Apps portáteis podem ser limpos** (não entram em `is_app_path`, D19), ao contrário de snap e flatpak.

**Medido (scan de `/`):**

| Categoria | Antes | Depois |
|---|---|---|
| Vídeos | 1,29 GB / 37.285 arquivos | 0,99 GB / **18** arquivos (somando D22) |
| Áudio | 122 arquivos (sons do IDE) | 0 |
| Documentos | 0,32 GB / 7.819 arquivos | 0,26 GB / 1.590 arquivos |
| Fotos | 0,31 GB / 5.700 arquivos | 0,29 GB / 4.574 arquivos |
| Walk de `/` | 2,20 a 2,41 s | 2,19 a 2,45 s (sem mudança) |

### D22 — `.ts` não é vídeo <a id="d22"></a>
**Decisão:** `.ts` saiu da tabela de vídeos e cai em Outros.

**Por quê:** nesta máquina, 37.267 dos 37.285 "vídeos" eram TypeScript (0,3 GB). MPEG-TS só aparece em gravações de TV/IPTV, raras no desktop.

**Descartado:** decidir pelo tamanho (`.ts` com mais de 1 MB seria vídeo). Exigiria passar o tamanho para o `classify` só para esse caso. Fica como opção se aparecerem gravações `.ts` de verdade.

### D23 — ⚠️ Mídia dentro de projetos de código <a id="d23"></a>
**Situação:** imagens e documentos de projetos seus (por exemplo `~/grupo-05-ihc/…/site/assets/images/*.jpg`) continuam contando como Fotos e Documentos.

**Por que ficou assim:** reconhecer a raiz de um projeto exige procurar `package.json`, `Cargo.toml` ou `.git` no disco, o que é I/O extra por diretório. Também é discutível: esses arquivos são seus, não de uma dependência.

**Impacto:** poucos MB.

---

## Lixeira e Docker

### D14 — Lixeira contada à parte <a id="d14"></a>
**Decisão:** `~/.local/share/Trash` fica em Outros, mas com um total próprio (`trash_bytes`, hoje 5,77 GB).

**Por quê:** **mover para a Lixeira não libera espaço.** O usuário precisa ver quanto está parado lá e ter a ação "Esvaziar". Depois de mandar arquivos para a Lixeira pelo app, os bytes deles passam para esse subitem.

### D15 — Docker via `docker system df` <a id="d15"></a>
**Decisão:** o uso do Docker é perguntado ao daemon e vira um subitem de Apps/Sistema, descontado do restante.

**Por quê:**
- `/var/lib/docker` é só de root e explicava 97 dos 106 GB "sem acesso".
- O daemon responde sem root para quem está no grupo `docker`.

**Detalhes:**
- O Docker informa tamanho lógico, não blocos alocados. Por isso o desconto é `min(docker, restante)`, para a barra não passar de 100%.
- Sem Docker, com o daemon parado ou sem permissão, o resultado é `None`, sem erro.
- Os tamanhos chegam como texto decimal (`"58.68GB"`) e são convertidos para bytes.

**Medido:** "sem acesso" caiu de 106 GB para 9,15 GB, e 74,9 GB do Docker podem ser liberados.

**Futuro:** Docker rootless e Podman, no backlog.

### D16 — Docker não bloqueia o scan <a id="d16"></a>
**Decisão:** `scan()` não chama o Docker. Quem chama aplica o resultado depois, com `ScanResult::apply_docker()`. No app, a consulta só começa depois do evento `Done`.

**Por quê:** foi medido:

| Situação | Tempo |
|---|---|
| `docker system df` sozinho | 5,2 s, mais que o walk inteiro |
| Os dois em paralelo | walk de 4 a 5 s, porque o daemon varre o disco junto |

Rodando em sequência, a barra fecha em cerca de 2 s e o card do Docker chega num evento próprio (`ScanEvent::Docker`) uns 5 s depois.

`apply_docker` pode ser chamado de novo, por exemplo depois de uma limpeza.

---

## Limpeza e segurança

### D17 — Lixeira sim, `rm` nunca <a id="d17"></a>
**Decisão:** o MVP inclui ações (escolha sua): "Abrir na pasta", "Mover para Lixeira" e "Esvaziar Lixeira" com confirmação.

**Por quê:** o objetivo original é liberar espaço, e só visualizar não resolve. Mandar para a Lixeira é reversível pelo Nautilus. O único apagamento definitivo é esvaziar a Lixeira, com um diálogo mostrando o tamanho.

### D18 — Limpeza do Docker sem volumes <a id="d18"></a>
**Decisão:** dois botões, cada um com confirmação:
- "Limpar build cache" → `docker builder prune -f`
- "Remover imagens não usadas" → `docker image prune -a -f`

Volumes só são exibidos, **sem botão**.

**Por quê:** build cache e imagens podem ser reconstruídos ou baixados de novo. Volumes guardam dados (bancos, uploads), e perdê-los é irreversível.

### D19 — Validação no Rust, não na UI <a id="d19"></a>
**Decisão:**
- `move_to_trash` só aceita caminhos que, depois de canonicalizados, estejam sob o `$HOME`, fora de pastas de apps gerenciados (`Classifier::is_app_path`: snap, flatpak, Steam) e presentes no último scan. Apps portáteis (AppImage, Electron descompactado) podem ir para a Lixeira (D21).
- `docker_prune` só executa comandos fixos, sem argumentos vindos da UI.
- As capabilities do Tauri expõem só os commands necessários.

**Por quê:** a UI é JavaScript, e qualquer coisa que chega pelo `invoke` deve ser tratada como entrada não confiável. A barreira precisa ficar onde o arquivo é de fato tocado.

---

## Ponte Tauri

### D24 — Ciclo do scan num serviço testável <a id="d24"></a>
**Decisão:** toda a lógica da ponte fica em `AppState::start_scan(opts, docker, send)` (`state.rs`), sem tipos do Tauri. Os commands só repassam para ele. A consulta ao Docker e o destino dos eventos são parâmetros.

**Por quê:** não há como clicar na janela num teste automatizado. Com o Docker falso e um `mpsc` no lugar do `Channel`, o `cargo test` cobre:
- a ordem dos eventos;
- o cancelamento;
- a recusa de dois scans ao mesmo tempo;
- o descarte de um resultado de Docker antigo.

**Detalhes:**
- `running` volta a `false` já no `Done`, para o usuário poder reescanear sem esperar os ~5 s do Docker.
- Para isso não causar corrida, cada scan tem uma `generation`. O Docker de um scan antigo, se responder depois, é descartado.
- `ScanEvent` ganhou `Cancelled`, que não estava na ARCHITECTURE, para a UI distinguir cancelamento de erro.

### D25 — Permissão explícita por command <a id="d25"></a>
**Decisão:** o `build.rs` declara os commands no `AppManifest` do Tauri, e `capabilities/default.json` libera cada um por nome (`allow-start-scan`, etc.). O `opener:default` do template foi removido.

**Por quê:** sem o manifesto, o Tauri v2 libera para a janela todo command registrado. Com ele, um command novo só fica acessível depois de entrar explicitamente na capability. Isso importa a partir da Etapa 6, quando houver commands que apagam coisas (D19). O `opener:default` permitia abrir URLs e arquivos arbitrários. Na Etapa 5, nem o `allow-reveal-item-in-dir` foi preciso (D31).

### D26 — Caminhos não UTF-8 serializados com perdas <a id="d26"></a>
**Decisão:** `FileEntry.path` e `DiskInfo.mount` são serializados com `to_string_lossy`. Bytes inválidos viram `�`.

**Por quê:** nomes de arquivo no Linux são bytes, não texto. O serde recusa um `PathBuf` não UTF-8, e um único arquivo assim entre os 200 maiores faria o evento `done` inteiro falhar.

**Consequência para a Etapa 6:** o caminho que volta da UI não é o real, então ações sobre esses arquivos precisam ser recusadas, o que a validação de D19 já faz ao exigir que o caminho conste no último scan.

### D27 — Motor otimizado também no `tauri dev` <a id="d27"></a>
**Decisão:** o `[profile.dev.package]` do workspace compila `scanner`, `jwalk`, `rayon` e `crossbeam` com `opt-level = 3`. O resto do app continua em debug.

**Por quê:** foi medido:

| Perfil | Walk de `/` |
|---|---|
| debug puro | 4,1 a 4,4 s |
| debug com o motor otimizado | 2,5 a 2,7 s |
| release | cerca de 2,2 s |

O app em desenvolvimento passa a se comportar como o real, e o build incremental do app não fica mais lento.

### D28 — SvelteKit em modo SPA <a id="d28"></a>
**Decisão:** manter o template SvelteKit gerado na Etapa 0 (`adapter-static`, `ssr = false`), em vez de trocar para Svelte puro como dizia o STACK_CHOICE.

**Por quê:** para um app de uma tela, a diferença é só de lugar: `routes/+page.svelte` faz o papel de `App.svelte` e `src/lib` é importado como `$lib`. Trocar o template custaria tempo sem ganho. O bundle é estático, sem servidor.

---

## Tela principal

### D29 — Barra ao vivo com faixa "ainda não contado" <a id="d29"></a>
**Decisão:** durante o scan, a barra mantém a escala do disco inteiro. As categorias crescem da esquerda, "Livre" fica fixo na ponta direita e o meio é uma faixa listrada animada com o que ainda não foi contado. No `done`, o restante entra em Apps/Sistema (D05) e a faixa some.

**Por quê:** o restante (`usado − escaneado`) só é conhecido no fim. A escala fixa faz cada segmento só crescer, sem saltos, e o fechamento dos 100% vira a própria animação final.

**Descartado:**
- Normalizar pelo escaneado até o momento: todas as larguras mudariam a cada evento e "pulariam" no fim.
- Esconder a barra até o `done`: o scan leva ~2 s, mas com cache frio são ~12 s sem nenhum retorno.

### D30 — Reescanear zera a barra; cancelar volta ao resultado anterior <a id="d30"></a>
**Decisão:** "Reescanear" relê `disk_info` e recomeça a barra do zero. Se o scan for cancelado, a tela volta ao último resultado completo, com o `DiskInfo` dele. Sem resultado anterior, fica o parcial, com a faixa "ainda não contado" parada.

**Por quê:** um scan cancelado não tem o restante, então a barra dele não soma 100%. Mostrar o resultado anterior mantém a promessa de D05 sempre que existe um. O `DiskInfo` volta junto para o "Livre" bater com as categorias daquele scan.

---

## Detalhe da categoria

### D31 — "Abrir na pasta" como command próprio <a id="d31"></a>
**Decisão:** o `reveal(path)` é um command do app. Ele só aceita caminhos que constem no último scan (`AppState::contains_file`) e então chama `tauri_plugin_opener::reveal_item_in_dir` pelo Rust. A janela não recebe nenhuma permissão do plugin opener.

**Por quê:** a permissão `opener:allow-reveal-item-in-dir` deixaria o JavaScript revelar qualquer caminho. O risco é baixo, mas D19 vale para todo caminho vindo da UI, e a mesma checagem será usada pelo `move_to_trash` na Etapa 6. De quebra, nomes não UTF-8 (D26) são recusados: o caminho com `�` não casa com o real.

**Detalhes:** o command é `async` porque a chamada D-Bus ao `FileManager1` bloqueia, e commands síncronos rodam na thread principal da janela.

**Descartado:** chamar o plugin direto do frontend com a permissão do opener.

### D32 — Docker com tamanho e liberável por tipo <a id="d32"></a>
**Decisão:** `DockerUsage` passou de 4 tamanhos e um liberável total para 4 `DockerItem { size, reclaimable, count }`, um por linha do `docker system df`.

**Por quê:** o card do Docker mostra cada tipo com o quanto dá para liberar. As ações da Etapa 6 são por tipo (build cache, imagens), e volumes, mesmo liberáveis para o Docker, nunca são removidos pelo app (D18). Um total único misturaria essas coisas.

