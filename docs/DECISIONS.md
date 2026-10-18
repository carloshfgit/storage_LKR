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
| [D13](#d13) | Sem leitura de conteúdo (magic bytes) — substituída em parte pela D37 | ✅ | 2 |
| [D14](#d14) | Lixeira contada à parte | ✅ | 2 |
| [D15](#d15) | Docker via `docker system df` | ✅ | 2 |
| [D16](#d16) | Docker não bloqueia o scan | ✅ | 2–3 |
| [D17](#d17) | Limpeza: Lixeira sim, `rm` nunca | ✅ | 6 |
| [D18](#d18) | Limpeza do Docker sem volumes | ✅ | 6 |
| [D19](#d19) | Validação de segurança no Rust, não na UI | ✅ | 3–6 |
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
| [D33](#d33) | Validação do `move_to_trash`: caminho inalterado e veredito na lista | ✅ | 6 |
| [D34](#d34) | Ações devolvem o resultado atualizado; liberado medido pelo `statvfs` | ✅ | 6 |
| [D35](#d35) | Abrir com o último scan e atualizar em segundo plano | ✅ | 7 |
| [D36](#d36) | Nome "Storage Looker" e pacote só `.deb` | ✅ | 7 |
| [D37](#d37) | Magic bytes só para arquivos sem extensão no `$HOME` | ✅ | 8 |
| [D38](#d38) | Docker rootless: socket fixo por daemon, sem contar duas vezes | ✅ | 9 |
| [D39](#d39) | Sugestões no walk; caches inteiros para a Lixeira | ✅ | 10 |
| [D41](#d41) | Lixeira com card próprio; itens pelo `id` do sistema | ✅ | 12 |
| [D42](#d42) | Detalhe do Docker só de leitura, com dicas de comandos | ✅ | 13 |
| [D43](#d43) | CSS dos componentes injetado pelo JS no dev | ✅ | 13 |
| [D44](#d44) | Esvaziar a Lixeira item por item; itens travados com comandos para o terminal | ✅ | 14 |

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

**Atualização (Etapa 8):** continua valendo para quase todos os arquivos. A exceção são os arquivos sem extensão no `$HOME` (D37).

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

---

## Ações de limpeza

### D33 — Validação do `move_to_trash` <a id="d33"></a>
**Decisão:** além do que D19 pede (consta no último scan, sob o `$HOME`, fora de `is_app_path`), o caminho canonicalizado precisa ser **igual** ao recebido, e arquivos que já estão na Lixeira são recusados. A mesma função (`check_trashable`) marca cada arquivo da lista com `trashable`, e a UI só mostra o checkbox nesses.

**Por quê:**
- Entre o scan e o clique, uma pasta pode virar link simbólico para fora do `$HOME`. Exigir o caminho inalterado fecha essa brecha sem precisar raciocinar sobre o destino do link. Como o walk não segue links, os caminhos do scan já são canônicos.
- Mandar para a Lixeira algo que já está nela criaria uma cópia dentro de `Trash/files`.
- Uma regra só para a lista e para a ação: a UI nunca oferece algo que o Rust vai recusar. A recusa continua acontecendo no Rust, para quem chamar o command direto pelo DevTools.

**Detalhes:** cada caminho é validado e movido sozinho. Um recusado não impede os outros, e o `TrashReport` traz o motivo de cada recusa.

### D34 — Ações devolvem o resultado atualizado <a id="d34"></a>
**Decisão:** `move_to_trash`, `empty_trash` e `docker_prune` atualizam o `ScanResult` no Rust e o devolvem inteiro. A UI troca o dela por ele, e a barra anima até os valores novos. O "liberado" de `empty_trash` e `docker_prune` é a diferença do `f_bavail` antes e depois.

**Por quê:**
- O Rust já guarda o resultado para validar as ações. Repetir a contabilidade em TypeScript abriria espaço para as duas versões divergirem.
- O `statvfs` é o que o `df` e o GNOME mostram. O tamanho que o Docker relata é lógico (D15), e o que a Lixeira guarda pode ter hardlinks, então somar tamanhos daria outro número.

**Descartado:** reescanear depois de cada ação. Seria exato, mas levaria ~2 s e zeraria a barra (D30) a cada clique.

---

## Polimento e pacote

### D35 — Abrir com o último scan e atualizar em segundo plano <a id="d35"></a>
**Decisão:** o app salva o último resultado em `~/.cache/storage-lkr/last.json` a cada mudança. Ao abrir, mostra esse resultado na hora e reescaneia **em segundo plano**: a barra continua no resultado anterior até o `done` e então anima até os valores novos. O "Reescanear" manual continua zerando a barra (D30).

**Por quê:**
- O scan com cache frio leva ~12 s, justamente na primeira abertura depois do boot. Com o resultado anterior, a tela é útil em menos de 1 s.
- Zerar a barra na abertura jogaria fora o cache que acabou de aparecer. Já o "Reescanear" é um pedido explícito, e ver a barra crescer confirma que o scan está andando.
- O `home` vai no arquivo à parte do `ScanResult`, para as ações validarem contra o mesmo `$HOME` da classificação.

**Detalhes:**
- O "Livre" não é relido no início do scan em segundo plano: ele precisa continuar batendo com as categorias na tela. O disco novo chega com o `done`.
- O detalhe já funciona com o cache, mas as ações esperam o scan terminar (D24, `running`).

**Descartado:** guardar o cache em `~/.local/share` (não é dado do usuário: pode ser apagado sem perda) ou no diretório do app derivado do `identifier` (o nome do diretório mudaria com o identificador).

### D36 — Nome "Storage Looker" e pacote só `.deb` <a id="d36"></a>
**Decisão:**
- Nome do produto e título da janela: **Storage Looker**, com o ícone `logo-storage-looker-sf.png`.
- Binário e pacote: `storage-looker`; crate `storage-looker` (lib `storage_looker_lib`); identificador `com.carloshf.storage-looker`.
- O diretório do cache continua `storage-lkr`, como no planejamento ("lkr" de looker).
- `bundle.targets = ["deb"]`.

**Por quê:** o nome com espaço fica só no que o usuário vê; arquivos e pacotes ficam sem espaço. Os outros formatos (AppImage, RPM) não são o alvo do MVP. O AppImage, em particular, baixa ferramentas extras durante o build.

---

## Pós-MVP

### D37 — Magic bytes só para arquivos sem extensão no `$HOME` <a id="d37"></a>
**Decisão:** o conteúdo só é lido quando o caminho já decidiu tudo menos a categoria final: arquivo regular, visível, dentro do `$HOME`, fora de build/dev, **sem extensão** e com pelo menos 4 KiB (regra 6b). São no máximo 32 bytes, comparados com uma tabela própria (`sniff.rs`). Sem assinatura conhecida, o arquivo fica em Outros, como antes.

**Por quê:**
- D13 continua certa para o milhão de arquivos de `/`. Nesta máquina só 1.068 arquivos chegam à regra 7 sem extensão, e só 417 deles têm 4 KiB ou mais. O walk de `/` ficou igual, dentro do ruído (2,24–2,42 s contra 2,10–2,62 s).
- Extensão desconhecida (`.xyz`) não é lida: quem pôs uma extensão quis dizer algo, e lê-las multiplicaria as leituras.
- Abaixo de 4 KiB, praticamente nada é mídia: são configs, scripts e marcadores. E o `open` custa o mesmo para 100 B e para 1 GB.
- `classify_by_path` devolve `Option`, para a regra ficar no classificador (puro e testável) e o I/O ficar no walk. Uma função só, em vez de um `needs_sniff` separado, evita repetir a passada pelos componentes do caminho.
- `O_NOATIME`: analisar o disco não deve mudar o horário de acesso dos arquivos. O kernel só aceita a flag para o dono, e para os outros a leitura segue sem ela.

**Descartado:**
- Crate `infer`: cobre centenas de formatos de que não precisamos e traz mais uma dependência para cerca de 20 assinaturas.
- Ler arquivos ZIP para distinguir docx/odt/epub: exigiria ler o diretório central, que fica no fim do arquivo. Isso é caro, e documentos de escritório quase sempre têm extensão.
- Reclassificar MPEG-TS (`0x47` a cada 188 bytes): é raro no desktop e fácil de confundir (D22).

### D38 — Docker rootless: socket fixo por daemon, sem contar duas vezes <a id="d38"></a>
**Decisão:**
- O app consulta até dois daemons, cada um pelo seu socket fixo: `/var/run/docker.sock` (sistema) e `$XDG_RUNTIME_DIR/docker.sock` (rootless). Ambos via `docker --host unix://…`.
- `~/.local/share/docker` (e `~/.local/share/containers`, do Podman) entram na regra 1: Apps/Sistema e nunca vão para a Lixeira.
- O walk soma à parte o que leu em `~/.local/share/docker`. Do total do rootless, só a parte que o walk **não** leu sai do restante.

**Por quê:**
- Sem a regra 1, as camadas do rootless apareciam como **Outros** (pasta oculta) e com checkbox de Lixeira. Apagar camadas à mão corrompe o armazenamento do Docker.
- O rootless mora no `$HOME`: o walk lê boa parte (arquivos do próprio usuário), mas não o que pertence a subuids mapeados, que cai no restante. Descontar o total do rootless do restante contaria duas vezes o que o walk já leu. Subtrair o lido é a correção mais simples que mantém a barra em 100%. Tamanho lógico contra alocado continua sendo uma aproximação, como no rootful (D15).
- Socket fixo em vez do contexto atual: o `dockerd-rootless-setuptool.sh` costuma rodar `docker context use rootless`. Com isso, o `docker` sem `--host` passaria a falar com o rootless, e a consulta "do sistema" devolveria o mesmo daemon. Fixar os dois deixa cada um com um papel só, e a UI só escolhe um enum.
- Sem socket, o `docker` nem é chamado: é o caso da maioria das máquinas, e não custa nada.

**Descartado:**
- `docker context ls` para descobrir os daemons: depende de configuração do usuário (nomes livres, contextos remotos por SSH/TCP que não são deste disco).
- `docker info --format '{{.DockerRootDir}}'` para achar a pasta de dados: mais uma chamada lenta por daemon. Vale a pena só quando houver mais de um disco (Etapa 11).
- Card do Podman: `podman system df` tem outro formato e não há Podman nesta máquina para validar. Por enquanto, a pasta dele só fica protegida.

**Limitações:**
- Um `data-root` personalizado no `daemon.json` do rootless não é detectado: a pasta conta como dado comum, e o desconto pode contar duas vezes, limitado ao restante.
- Um rootful com `DOCKER_HOST` ou socket em outro caminho não é encontrado.

### D39 — Sugestões no walk; caches inteiros para a Lixeira <a id="d39"></a>
**Decisão:**
- "Downloads antigos" e "Caches" são somados no próprio walk. São dois testes de prefixo por entrada e o mtime que o stat já trouxe, sem nenhuma leitura extra.
- **Downloads antigos:** arquivos sob a pasta de Downloads do `user-dirs.dirs` sem modificação há mais de 90 dias, contados a partir do início do scan.
- **Caches:** agregados por item direto de `~/.cache` (normalmente um app). Os 20 maiores vão para o resultado e são movidos **inteiros** para a Lixeira, com confirmação.
- **"Maiores arquivos":** sai dos top-N das categorias que já existem, filtrados pelo `check_trashable`. Não há heap novo.

**Por quê:**
- Um scan à parte só para as sugestões custaria outro walk do `$HOME` (cerca de 1,5 s). No walk, o custo fica no ruído: `/` levou 2,12–2,29 s, igual a antes.
- A pasta de Downloads tem nome localizado (`~/Transferências` em algumas instalações), por isso vem do `user-dirs.dirs`, o mesmo arquivo do Nautilus. `$HOME` como valor desliga a pasta no padrão do `xdg-user-dirs`, e a sugestão some em vez de pegar a pasta pessoal inteira.
- 90 dias pelo mtime: o atime não é confiável com `relatime`/`noatime`, e um download não modificado há 3 meses é um bom candidato. Mesmo assim, só **sugere**: nada é marcado sozinho.
- `SuggestedFile` guarda a categoria porque um download antigo pode não estar no top-N da categoria dele. Sem isso, mandar o arquivo para a Lixeira não saberia de qual segmento tirar os bytes. Reclassificar pelo caminho erraria nos arquivos reconhecidos pelo conteúdo (D37).
- Cache por item de `~/.cache`, e não por arquivo: os arquivos de cache são muitos e pequenos, e o que o usuário entende é "o cache do Chrome". Vai para a Lixeira e não é apagado, como tudo no app (D17). Restaurar é possível, e o espaço só volta ao esvaziar, o que a UI avisa.
- A validação exige **filho direto** de `~/.cache`, com o caminho canônico igual ao do scan (D33). Nem a pasta inteira (que tem o cache do próprio app e de serviços em uso) nem algo mais fundo pode ser pedido pela UI.

**Descartado:**
- Apagar caches permanentemente (como o "Limpar cache" de alguns apps): contraria D17, e um cache em uso pode ser necessário para fechar o app direito.
- Contar `~/.cache/thumbnails` à parte: já é um item direto de `~/.cache` e aparece na lista como qualquer outro.
- Um heap global para "maiores arquivos": os 200 maiores de cada categoria já cobrem os 50 maiores do disco.

**Limitações:**
- Um AppImage dentro de `~/.cache` é contado em Apps/Sistema (regra 3). Ao mover o item inteiro, os bytes dele entram em `trash_bytes`, mas não saem de Apps/Sistema até o próximo scan.
- O cache em `$XDG_CACHE_HOME`, fora de `~/.cache`, não é considerado.

### D41 — Lixeira com card próprio; itens pelo `id` do sistema <a id="d41"></a>
**Decisão:**
- A Lixeira ganha um card na tela principal e um detalhe com os itens: restaurar, apagar os selecionados e esvaziar tudo. O card de Outros vira um atalho para ele.
- O walk soma bytes e arquivos por item direto de `~/.local/share/Trash/files` (`trash_items`), com o mesmo agregador dos caches (D39). O detalhe junta isso à listagem do sistema (`trash::os_limited::list`), que traz o nome, a pasta original e a data.
- A UI identifica cada item pelo `id` que o Rust deu (o caminho do `.trashinfo`). Restaurar e apagar relêem a listagem e só agem sobre itens presentes nela.
- Restaurar **nunca sobrescreve**: um nome já ocupado no local original volta recusado com o motivo.
- O item restaurado volta para a categoria de `classify(caminho original)`, fora do top-N.

**Por quê:**
- A Lixeira é o "liberável" mais óbvio do disco (5,77 GB nesta máquina) e ficava escondida dentro de Outros. Como no iOS ("Apagados recentemente"), ela merece um lugar próprio, e restaurar dá ao usuário uma saída que antes exigia o Nautilus.
- Por `id` e não por caminho: um caminho vindo da UI poderia apontar para qualquer coisa, e o `restore_all`/`purge_all` do crate agem sobre o que recebem. Com o `id` conferido contra a listagem relida, o que chega ao crate é sempre um item real da Lixeira do usuário, montado no Rust (D19).
- Tamanho do scan, não do `metadata` do crate: o crate dá o tamanho aparente de um arquivo e só a contagem de entradas de uma pasta. O walk já tem o espaço alocado (D06), sem I/O extra. O que o scan não viu é medido na hora com `tree_size`, com as mesmas regras.
- Um item que o app moveu depois do scan é achado pelo caminho original (`moved`), porque o crate pode renomeá-lo dentro da Lixeira (`foto.2.jpg`) e não devolve o nome.
- Sem sobrescrever: é o padrão do crate e do Nautilus, e a alternativa apagaria um arquivo que o usuário recriou.
- Classificar pelo caminho original acerta o caso comum (uma foto volta para Fotos) sem reler o arquivo. Uma pasta restaurada vai inteira para a categoria do nome dela (normalmente Outros), e o próximo scan distribui.

**Descartado:**
- Mandar caminhos da UI, como no `move_to_trash`: a validação teria de reimplementar as regras do freedesktop (pasta `files/`, `.trashinfo` correspondente, lixeiras de outros discos).
- Ler os `.trashinfo` no walk: seria I/O por item no caminho quente, só para o detalhe, que já relê a listagem ao abrir.
- "Restaurar substituindo": perde dados sem desfazer.

**Limitações:**
- Bytes restaurados não entram no top-N da categoria até o próximo scan, e uma pasta restaurada não é redistribuída entre as categorias.
- Itens de lixeiras de outros discos aparecem na lista (o crate lista todas), mas não estão na barra, que é do disco do sistema (D09). Eles vêm marcados como medidos agora.

### D42 — Detalhe do Docker só de leitura, com dicas de comandos <a id="d42"></a>
**Decisão:**
- Cada card Docker ganha "Ver detalhes", que abre a lista item a item do daemon: volumes, imagens, containers e build cache, do `docker system df -v --format json`.
- A lista é consultada **sob demanda**, por um command próprio (`docker_details(mode)`), e não guardada no `ScanResult`.
- A tela é **só de leitura**. Para liberar espaço, ela mostra dicas montadas a partir da lista ("52 volumes anônimos sem uso, 16,4 GB"), cada uma com o comando para o usuário rodar no terminal e um botão "Copiar".
- D18 continua valendo: o app não apaga volumes nem containers, e não ganhou botão novo.

**Por quê:**
- O card só dizia "Volumes 16,7 GB". Nesta máquina, a lista mostra que 52 dos 56 volumes são anônimos e sem uso (16,4 GB), sobras de containers recriados, e que os 4 com nome (bancos de projetos) somam só 0,33 GB. Sem a lista, não dava para saber o que é lixo e o que é dado.
- Sob demanda: o `-v` leva o mesmo tempo do `df` simples (cerca de 4,5 s), mas a saída tem cerca de 125 KB (252 entradas de build cache, labels dos containers). Guardar isso em cada resultado aumentaria o cache em disco e o evento `docker` para algo que só se vê ao abrir o detalhe. Trocar a consulta do scan pela `-v` também não foi feito: o card e a barra continuam com o `df` simples, que já está testado e é o que o `docker_prune` relê.
- Dicas em vez de botões: remover um volume com nome ou um container pode perder dados, e o usuário é quem sabe se um banco ainda serve. O comando copiado deixa a decisão com ele, no terminal, onde o próprio `docker` pede confirmação. Para os anônimos, a dica aponta o `docker volume prune`, que desde o Docker 23 remove só volumes anônimos sem uso.
- **Versão do daemon:** o detalhe também roda `docker version`. Só no Docker 23 ou mais novo a dica dos anônimos usa `docker volume prune`, que desde essa versão remove só os anônimos sem uso. Em versão anterior ou desconhecida, o mesmo comando apagaria os volumes com nome. Por isso a dica filtra pelo nome: `docker volume ls -qf dangling=true | grep -E '^[0-9a-f]{64}$' | xargs -r docker volume rm`. Antes da 23 não havia a label `com.docker.volume.anonymous`, então um volume também conta como anônimo quando o nome é um id de 64 dígitos hexadecimais.
- No rootless, os comandos levam `DOCKER_HOST=unix://$XDG_RUNTIME_DIR/docker.sock`, pelo mesmo motivo do socket fixo (D38). A variável vale também para os plugins (`compose`, `buildx`), o que o `--host` não garante.
- A saída do `-v` passa dos 64 KiB do pipe. A execução do `docker` passou a ler a saída numa thread enquanto espera o processo; esperar o exit antes de ler travaria o comando até o timeout.

**Descartado:**
- Botão "Remover volumes anônimos sem uso" (`docker volume prune -f`): seguro no Docker 23+, mas num Docker mais antigo o mesmo comando remove também os volumes com nome. Fica para uma etapa própria; a versão do daemon já é lida para as dicas.
- Remoção individual de volume, imagem ou container pela UI: exigiria mandar nomes da UI para a linha de comando, o que D19 evita, e reabriria D18.
- Mostrar cada camada do build cache agrupada por Dockerfile: o `df -v` não diz de qual projeto é cada entrada. A lista vem por tamanho, com a descrição do passo e o último uso.

**Limitações:**
- Os tamanhos são os do Docker (tamanho lógico, D15), e o de cada imagem inclui camadas compartilhadas, que a tela indica.
- Abrir o detalhe durante um scan em segundo plano roda o `docker` junto com o walk, o que o deixa um pouco mais lento (§6.1).
- Num Docker antigo, sem suporte a `system df -v --format json`, a tela mostra o erro e o card continua como antes.

### D43 — CSS dos componentes injetado pelo JS no dev <a id="d43"></a>
**Decisão:** no `vite dev`, o `svelte.config.js` usa `vitePlugin.emitCss = false`. Assim o compilador gera `css: "injected"`: cada componente insere o seu `<style>` com escopo pelo próprio JS. No build (`NODE_ENV=production`), o CSS continua extraído em arquivo, como antes.

**Por quê:**
- Com `emitCss` ligado, o JS de cada componente importa um módulo virtual, `X.svelte?svelte&type=style&lang.css`. O plugin só sabe respondê-lo com o CSS que guardou ao compilar o componente. Se o pedido chega antes disso, ele registra `failed to load virtual css module` e devolve `undefined`. O Vite então carrega o arquivo `.svelte` inteiro e o serve como CSS.
- O navegador aproveita o que consegue desse texto. As regras saem **sem o escopo** do Svelte: o `button { width: 100%; display: grid }` e o `.card { margin-top: 14px; … }` do `Suggestions.svelte` e do `TrashCard.svelte` passam a valer para todos os botões e cards da página. Foi o que apareceu no `tauri dev`: botões esticados e o card da Lixeira com o espaçamento errado.
- O erro fica: o Vite guarda essa versão no grafo de módulos (o ETag tem o tamanho do `.svelte` cru) e a entrega até o arquivo mudar. Recarregar a janela ou apagar `node_modules/.vite` não resolve.
- Reproduzido pedindo o módulo de CSS antes do componente num servidor novo. O pedido antecipado aconteceu no webview do Tauri (WebKitGTK) do `tauri dev`, mas não se repetiu com o Chrome nem com o WebKitGTK isolados. Como o gatilho depende do momento e do cache do webview, a correção tira o módulo virtual do dev, em vez de tentar evitar o pedido.

**Descartado:**
- `css: "injected"` também no build: funcionaria (a CSP é `null`), mas troca um arquivo CSS em cache por estilos inseridos em tempo de execução, sem ganho. O problema é só do servidor de dev.
- Esperar uma correção do `@sveltejs/vite-plugin-svelte`: a 7.3.1 já é a mais nova, e não há relato do problema.

**Limitações:**
- O dev e o build diferem em como o CSS chega à página. As regras e o escopo são os mesmos; `npm run tauri build` continua sendo a conferência final.
- Um shell que exporte `NODE_ENV=production` liga o `emitCss` também no dev.

### D44 — Esvaziar a Lixeira item por item; itens travados com comandos para o terminal <a id="d44"></a>
**Decisão:**
- `empty_trash` apaga item por item, como o `purge_from_trash`. Um item que falha volta em `rejected` com o motivo, e os outros são apagados. O retorno passou de `Cleanup` para `PurgeReport`.
- Na listagem, `locked_dirs` acha os itens com pastas que o usuário não consegue esvaziar. Eles aparecem como "travados", num card com o motivo e os comandos para o terminal. Os mesmos comandos voltam no recusado quando apagar falha.
- O app **não** roda os comandos: mostra e copia. Para pastas do root, sugere `sudo rm -rf` e, sem sudo, um container do Docker que apaga pela pasta montada. Para pastas do próprio usuário sem escrita, `chmod -R u+w`.
- As sugestões dependem do que o usuário tem (`Hints`):
  - O comando pelo Docker só aparece se o último scan achou o Docker **do sistema** (rootful). No rootless e no Podman, o root do container é o próprio usuário e o comando falharia igual.
  - Quem não está nos grupos `sudo`, `admin` ou `wheel` (lidos do `/etc/group`) vê que o `sudo` exige a senha de um administrador.
  - Um item com nome fora do UTF-8 não ganha comando: o `�` faria o caminho copiado não corresponder ao real.
- O card dos itens travados fica **recolhido**, com um convite para abrir.

**Por quê:**
- Nesta máquina, "Esvaziar Lixeira" falhava inteiro com `PermissionDenied` em `Trash/files/frontend`. O `purge_all` de todos os itens parava no primeiro erro, a UI dizia só "A limpeza falhou" e a barra não era atualizada, mesmo que parte já tivesse sido apagada. Seis itens da Lixeira (projetos `frontend` e `backend`) tinham 129 pastas do root: `.next`, `node_modules`, `.pytest_cache` e `__pycache__`, criadas por containers que rodam como root numa pasta montada.
- Mandar para a Lixeira funcionou porque mover a pasta de cima exige permissão só no `$HOME`. Apagar exige `wx` em cada pasta que contém algo, e nas do root o usuário não tem.
- Item por item: o usuário pediu para esvaziar, e um item travado não deveria impedir os outros. O liberado continua medido pelo `statvfs`, então conta também o que foi apagado dentro de um item que falhou no meio.
- Mostrar em vez de executar: rodar `sudo` pediria senha numa janela que o app não controla, e apagar como root pelo app abriria um caminho de escalada que o resto do projeto evita (D19). O comando copiado deixa a decisão com o usuário. Os caminhos vêm da listagem do sistema, nunca da UI, e entram entre aspas simples.
- Detectar ao listar, e não só quando falha: percorrer a Lixeira levou 25 ms (5.915 entradas). O usuário vê antes de confirmar quais itens vão ficar, sem precisar tentar. A conferência de permissão segue a regra do kernel (dono, grupo, outros), e uma pasta vazia sem escrita não trava nada, porque quem a remove é a pasta de cima.

**Descartado:**
- `pkexec rm -rf` pelo app: resolveria num clique, mas apagar como root a partir da UI é exatamente o que D19 evita. Fica para o modo privilegiado do backlog, se um dia existir.
- Contar as pastas travadas no walk (guardando no `TrashEntry`): mudaria o formato do cache por algo que a listagem calcula na hora, e ficaria velho entre os scans.
- Pular os itens travados sem tentar: o que é do usuário dentro deles seria apagado na tentativa, e o "liberado" medido mostra isso.

**Limitações:**
- Um item que falha no meio fica parcialmente apagado. Ele continua na lista com o tamanho do scan até o próximo scan.
- A checagem não considera ACLs nem o atributo imutável (`chattr +i`). Nesses casos, o recusado vem com o erro do sistema, sem comandos.
- O comando do Docker não funciona se o caminho da Lixeira tiver `:` (separador do `-v`); o do `sudo` continua valendo.
- Ser de um grupo de administradores não garante o `sudo` (o sudoers pode ser outro), e o contrário também vale. É só para escolher o texto; o comando é o mesmo.

