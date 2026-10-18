# PLAN — Etapas de desenvolvimento

Cada etapa termina com algo **executável e verificável**. Detalhes técnicos estão em [ARCHITECTURE.md](ARCHITECTURE.md), e a justificativa da stack em [STACK_CHOICE.md](STACK_CHOICE.md). O porquê de cada decisão está em [DECISIONS.md](DECISIONS.md).

**Escopo do MVP:**
- Barra com 7 segmentos: Fotos, Vídeos, Áudio, Documentos, Apps/Sistema, Outros e Livre.
- Detalhe por categoria.
- Abrir na pasta, mover para a Lixeira e esvaziar a Lixeira com confirmação.

---

## Etapa 0 — Ambiente
- [x] Instalar Rust: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- [x] Instalar as dependências do sistema:
  ```bash
  sudo apt install build-essential curl wget file libwebkit2gtk-4.1-dev libxdo-dev \
    libssl-dev libayatana-appindicator3-dev librsvg2-dev
  ```
- [x] Gerar o projeto: `npm create tauri-app@latest` (Svelte + TypeScript), com saída na raiz do repositório.
- [x] Converter para workspace Cargo (`[workspace] members = ["crates/scanner", "src-tauri"]`) e criar o `crates/scanner` vazio.
- [x] `git init` + `.gitignore` (`target/`, `node_modules/`, `dist/`).

**Pronto quando:** `npm run tauri dev` abre a janela padrão.

---

## Etapa 1 — Motor de scan (CLI)
- [x] `model.rs`: `Category`, `FileEntry`, `CategoryStats`, `ScanResult`.
- [x] `disk.rs`: `disk_info("/")` via `rustix::fs::statvfs`.
- [x] `walk.rs`:
  - jwalk paralelo, sem cruzar filesystem (`st_dev`), sem seguir symlinks
  - `st_blocks * 512` e dedupe de hardlinks
  - top-N com heap
  - progresso com rate-limit e cancelamento
- [x] Na Etapa 1, `classify` é provisório e só olha a extensão.
- [x] `bin/scan.rs`: imprime a tabela por categoria, o restante, os diretórios negados e o tempo.

**Pronto quando:**
- `cargo run --release -p scanner --bin scan` termina e total + restante bate com o `df /` (±1%).
- Scan apontado para `$HOME` ≈ `du -sx ~` (±1%).
- `/` com cache quente leva **menos de 5 s**.

**Resultado (17/10/2026, SSD, 4 núcleos):**

| Medida | Scanner | Referência |
|---|---|---|
| `$HOME` | 56 397 565 952 B, 742 mil arquivos, 1,45 s | `du -sxB1 ~` = 56 397 565 952 B (idêntico) |
| `/` (sem root) | 79,66 GB, 1,07 milhão de arquivos | `du -sxB1 /` = 79,66 GB (diferença de 86 KB) |
| `/`, cache quente | 2,1 a 2,6 s | meta < 5 s |
| `/`, cache frio | 11,9 s | — |

Total + restante = `df` por construção. O restante (106 GB) é real: são diretórios só de root, principalmente o Docker (`docker system df`: cerca de 97 GB entre imagens, volumes e build cache). 8 threads (2× núcleos) foi o melhor valor; 4 e 16 ficaram iguais ou piores.

---

## Etapa 2 — Classificação definitiva
- [x] Implementar as 6 regras ordenadas e a tabela de extensões de ARCHITECTURE §5.
- [x] Testes unitários (pelo menos estes casos):

| Caminho | Esperado |
|---|---|
| `/usr/share/icons/x.png` | Apps/Sistema |
| `~/.cache/mozilla/a.jpg` | Outros |
| `~/.local/share/Trash/files/v.mp4` | Outros (conta em `trash_bytes`) |
| `~/Vídeos/Férias.MKV` | Vídeos |
| `~/snap/spotify/foo.mp3` | Apps/Sistema |
| `~/Downloads/ubuntu.iso` | Outros |
| `~/Documentos/cv.pdf` | Documentos |

- [x] `docker.rs` (ARCHITECTURE §6.1): `docker_usage(timeout)` roda `docker system df --format json` com timeout de 20 s e devolve `Option<DockerUsage>`. O `scan()` não espera por ele.
- [x] Descontar o Docker do restante com `ScanResult::apply_docker`: `docker_bytes = min(Σ docker, restante)`.
- [x] Testes do parser de tamanhos (`"58.68GB"`, `"113.1MB"`, `"0B"`, `"39.88GB (68%)"`) e do caso sem Docker (binário ausente ou daemon inacessível vira `None`, sem erro).
- [x] O CLI mostra a linha "Docker" separada de "Restante (sem acesso)".

**Pronto quando:**
- `cargo test -p scanner` passa, e o CLI mostra uma distribuição plausível na máquina real.
- Nesta máquina, o CLI mostra cerca de 97 GB de Docker, e o "sem acesso" cai de 106 GB para menos de 15 GB.

**Resultado (17/10/2026):**

| Linha | Antes (Etapa 1) | Agora |
|---|---|---|
| Fotos | 1,35 GB | 0,31 GB (os JPG/PNG de caches e de `/usr` saíram) |
| Apps/Sistema | 0 GB | 130,2 GB = 23,9 escaneado + **97,15 Docker** + 9,15 reservado/sem acesso |
| Outros | 75,6 GB | 54,0 GB, dos quais 5,77 GB na Lixeira |
| "Sem acesso" | 106,3 GB | **9,15 GB** |

- 21 testes passam (`cargo test -p scanner`).
- Walk de `/` sem Docker: 2,0 a 2,5 s, sem perda com as novas regras.
- `docker system df` sozinho: cerca de 5,2 s. Junto com o walk, os dois disputam o disco (walk de 4 a 5 s, total de cerca de 6,5 s). Na Etapa 3, a consulta ao Docker vai rodar depois do `Done`.
- **Refinamento (DECISIONS D21–D23):**
  - Regras de app portátil (AppImage, `~/Applications`, `resources/app` do Electron) e de build/dev (`node_modules`, `target/debug|release`, `__pycache__`, `site-packages`).
  - `.ts` saiu de Vídeos.
  - Resultado: Vídeos caiu de 37.285 para 18 arquivos, e o tempo do walk não mudou.
  - 28 testes passam.
  - Resta como limitação (D23): mídia de projetos próprios conta como mídia.

---

## Etapa 3 — Ponte Tauri
- [x] `state.rs`:
  - `AppState`, gerenciado como `Arc`;
  - `ScanEvent` (`progress`, `done`, `docker`, `cancelled`, `error`);
  - ciclo do scan numa thread, com o Docker consultado depois do `done` e descartado se for de um scan antigo (`generation`).
- [x] `commands.rs`: `disk_info`, `start_scan` (Channel), `cancel_scan`, `category_files`.
- [x] `build.rs` + `capabilities/default.json`: cada command exige `allow-<command>`, e só esses 4 estão liberados. O `opener:default` foi removido.
- [x] `src/lib/api.ts` com os tipos espelhando `ScanEvent`/`ScanResult`.
- [x] `routes/+page.svelte` provisória, com Escanear/Cancelar, totais ao vivo, maiores por categoria e log dos eventos (também no `console.log`).
- [x] Caminhos não UTF-8 serializados com perdas, e motor otimizado no perfil `dev`.

**Pronto quando:** o DevTools da janela mostra eventos `progress` chegando e um `done` final, e `cancel_scan` interrompe o scan.

**Resultado (17/10/2026):**
- 4 testes da ponte, com Docker falso, cobrem:
  - `progress`* → `done` → `docker`;
  - cancelar logo após iniciar e depois escanear de novo;
  - recusa de um segundo scan simultâneo;
  - descarte do Docker de um scan antigo.
- Workspace: 33 testes passam, clippy limpo, `npm run check` sem erros.
- `npm run tauri dev` compila e abre a janela sem erros.
- Walk no perfil `dev`: de 4,1–4,4 s para 2,5–2,7 s com o motor otimizado.
- ⏳ **Falta a conferência visual** (clicar na janela e olhar o DevTools), que precisa ser feita por você.

---

## Etapa 4 — Tela principal
- [x] `theme.css`: tokens de cor, claro/escuro e tipografia.
- [x] `format.ts`: bytes decimais em pt-BR.
- [x] Header com nome do disco e "X GB de Y GB usados".
- [x] `StorageBar`: 7 segmentos com `Tween`, que crescem ao vivo durante o scan, com "Livre" desenhado logo após `disk_info`.
- [x] `Legend` ordenada por tamanho e botão "Reescanear".
- [x] Estados: escaneando (com contador de arquivos), pronto, cancelado e erro.

**Pronto quando:** a barra anima durante o scan, soma exatamente 100% no fim e alterna corretamente entre tema claro e escuro.

**Resultado (17/10/2026):**
- A barra mantém a escala do disco durante o scan, com uma faixa listrada "ainda não contado" que some no `done` (DECISIONS D29). Cancelar volta ao último resultado completo (D30).
- Apps/Sistema mostra na legenda o que inclui: Docker e reservado/sem acesso ("consultando Docker…" até o evento `docker`).
- `npm run check` sem erros, `npm run build` ok e `format.ts` conferido ("5,77 GB", "12,4 GB", "155 GB", 999,7 MB → "1,00 GB", "1.070.000").
- ⏳ **Falta a conferência visual** (animação, 100% no fim, tema claro e escuro), que precisa ser feita por você.

---

## Etapa 5 — Detalhe da categoria
- [x] `CategoryDetail` + `FileRow`: top-N ordenado por tamanho, com alternância para ordenar por data.
- [x] "Abrir na pasta" via `reveal` (`tauri-plugin-opener`), validado no Rust (DECISIONS D31).
- [x] Cards especiais: "Docker" e "Reservado / sem acesso" (Apps/Sistema) e "Lixeira" (Outros).
- [x] O card Docker mostra imagens, containers, volumes e build cache, cada um com tamanho e quanto é liberável (D32). Quando não há Docker, o card não aparece.

**Pronto quando:** clicar no segmento "Vídeos" lista os maiores vídeos, e "Abrir na pasta" abre o Nautilus com o arquivo selecionado.

**Resultado (17/10/2026):**
- O detalhe abre pelo segmento da barra ou pela linha da legenda, só com um resultado completo. Volta pelo botão ou `Esc`, sem refazer a animação da barra.
- `reveal` recusa caminhos fora do último scan, inclusive nomes não UTF-8. Novo teste da ponte cobre isso.
- Workspace: 34 testes passam, clippy limpo, `npm run check` sem erros.
- ⏳ **Falta a conferência visual** (clicar em "Vídeos" e em "Abrir na pasta"), que precisa ser feita por você.

---

## Etapa 6 — Ações de limpeza
- [x] `move_to_trash` com validação no Rust (canônico, sob `$HOME`, fora de apps, presente no último scan), usando o crate `trash`.
- [x] Seleção múltipla, rodapé com o total e atualização da barra: bytes saem da categoria e entram em "Lixeira".
- [x] `empty_trash` com diálogo de confirmação ("Apagar permanentemente X GB?"), seguido de nova leitura de `disk_info`.
- [ ] Teste manual: tentar mandar para a lixeira um caminho fora de `$HOME` via DevTools precisa ser rejeitado.
- [x] `docker_prune` no card Docker, com diálogo de confirmação mostrando o tamanho:
  - "Limpar build cache" → `docker builder prune -f`
  - "Remover imagens não usadas" → `docker image prune -a -f` (avisar que vão precisar ser baixadas de novo)
  - Volumes: só exibidos, **sem botão**, porque guardam dados.

**Pronto quando:**
- O arquivo aparece na Lixeira do Ubuntu e pode ser restaurado pelo Nautilus.
- Esvaziar a Lixeira aumenta o "Livre" na mesma quantidade vista no `df`.
- Limpar o build cache do Docker aumenta o "Livre" no valor que o `df` confirma, e o card Docker é atualizado.

**Resultado (17/10/2026):**
- As ações ficam em `state/cleanup.rs` e recebem a Lixeira, o `docker` e o `statvfs` como parâmetros. 8 testes com pasta temporária e fakes cobrem:
  - mover um arquivo válido e recusar, cada um com o motivo, os de fora do `$HOME`, os de app (`~/snap`), os que já estão na Lixeira e os inventados;
  - arquivo apagado depois do scan, pasta trocada por link simbólico e falha da própria Lixeira;
  - o `trashable` da lista, a recusa durante um scan, a confirmação obrigatória, o liberado medido pelo disco e a atualização do card Docker.
- Validação mais estrita que a planejada: o caminho canonicalizado precisa ser igual ao do scan (DECISIONS D33). Cada ação devolve o resultado atualizado (D34).
- Workspace: 46 testes passam, clippy limpo, `npm run check` sem erros.
- ⏳ **Falta a conferência manual**, que precisa ser feita por você:
  1. Mover um arquivo de teste para a Lixeira e restaurá-lo pelo Nautilus.
  2. No DevTools: `await window.__TAURI_INTERNALS__.invoke("move_to_trash", { paths: ["/etc/hostname"] })` deve voltar com `moved: 0` e o motivo em `rejected`.
  3. Esvaziar a Lixeira e comparar o liberado com o `df -B1 /` antes e depois.
  4. Limpar o build cache do Docker e conferir o mesmo.

---

## Etapa 7 — Polimento e pacote
- [x] Cache do último scan em `~/.cache/storage-lkr/last.json`: abre instantâneo e reescaneia em background (DECISIONS D35).
- [x] Ícone do app, título da janela e tamanho mínimo: "Storage Looker", 820×680, mínimo 560×480 (D36).
- [x] Animações finas: entrada do detalhe e hover nos segmentos com tooltip.
- [x] `npm run tauri build` → `.deb` → `sudo apt install ./target/release/bundle/deb/*.deb`.

**Pronto quando:** o app aparece no menu do GNOME, abre em menos de 1 s mostrando o último scan e atualiza sozinho.

**Resultado (17/10/2026):**
- Cache salvo a cada mudança do resultado (`done`, `docker` e ações) e carregado por `AppState::with_cache`. Os testes cobrem o caminho completo (scan → cache → app novo com o mesmo resultado, Docker e `$HOME`) e o cache ausente, corrompido ou de outra versão.
- `ScanResult` ganhou `finished_at`, para a UI mostrar "análise de há 2 h".
- Ícones gerados com `npx tauri icon logo-storage-looker-sf.png`. Pacote `storage-looker`, binário `/usr/bin/storage-looker`, categoria Utilitários.
- Workspace: 49 testes passam, clippy limpo, `npm run check` sem erros.
- ⏳ **Falta a conferência**, que precisa ser feita por você: instalar o `.deb`, abrir pelo menu do GNOME e ver o último scan aparecer na hora, com o "Atualizando…" em seguida.

---

## Etapa 8 — Classificação por magic bytes (backlog 5)
Hoje um arquivo visível no `$HOME` sem extensão cai em Outros (regra 7). D13 descartou ler conteúdo porque custaria um `open` + `read` por arquivo. Aqui a leitura vale **só** para quem chegaria à regra 7 sem extensão, e o custo é medido.
- [x] Medir antes quantos arquivos do `$HOME` chegam à regra 7 sem extensão. Se forem poucos milhares, seguir.
- [x] `classify.rs`: `classify` continua pura. `Classifier::classify_by_path(path)` devolve `None` quando o caminho cairia na regra 7 **sem extensão**: dentro do `$HOME`, visível e fora de build/dev. Arquivos com extensão desconhecida ficam fora. Na implementação, virou uma função só, em vez de um `needs_sniff` à parte, para não repetir a passada pelos componentes.
- [x] Novo `sniff.rs`: `sniff(path) -> Option<Category>` lê no máximo 32 bytes e compara com uma tabela estática de assinaturas, sem crate novo:
  - Fotos: JPEG `FF D8 FF`, PNG, GIF, WebP (`RIFF…WEBP`), HEIC/AVIF (`ftyp heic|heix|avif`)
  - Vídeos: MP4/MOV (`ftyp isom|mp42|qt  `…), Matroska/WebM `1A 45 DF A3`, AVI (`RIFF…AVI `)
  - Áudio: `ID3`/MPEG frame, `fLaC`, `OggS`, WAV (`RIFF…WAVE`), M4A (`ftyp M4A `)
  - Documentos: `%PDF`
  - ZIP (docx/odt), ELF, scripts e afins: `None`, ou seja, continua em Outros
- [x] Ignorar arquivos menores que 4 KiB, que são raramente mídia e caros proporcionalmente. Erro de leitura (`EACCES`) também vira `None`, sem contar como dir negado.
- [x] `walk.rs`: em `process_read_dir`, quando `classify_by_path` devolve `None` e a entrada é um arquivo regular, chamar `sniff`. A leitura roda nas threads do rayon, junto com o `symlink_metadata`.
- [x] `bin/scan`: `--no-sniff` para comparar, e uma linha "reclassificados por conteúdo: N arquivos, X GB".
- [x] Testes:
  - `sniff` com cabeçalhos sintéticos de cada assinatura, arquivo curto, vazio e sem permissão;
  - walk numa pasta temporária: `~/foto` (JPEG sem extensão) vai para Fotos, `~/.cache/foto` continua em Outros e `~/proj/node_modules/x` continua em Outros.
- [x] Docs: ARCHITECTURE §5 (nova regra 6b e o "não há leitura de conteúdo" reescrito); DECISIONS D37, que atualiza D13 (por que só sem extensão, limite de 4 KiB, tabela própria em vez do crate `infer`).

**Pronto quando:**
- Uma foto renomeada sem extensão no `$HOME` aparece em Fotos.
- O walk de `/` com cache quente continua **< 5 s**, com no máximo +10% em relação ao `--no-sniff`.
- `du`/`df` continuam batendo, porque só a categoria muda, não os bytes.

**Resultado (17/10/2026):**
- Medido antes: 1.068 arquivos visíveis no `$HOME` sem extensão (5,46 GB), dos quais 417 com 4 KiB ou mais. Vários estão em `target/`, que a regra 5 já tira. São poucas leituras.
- `ScanResult` ganhou `sniffed_files` e `sniffed_bytes`, e o `cache::VERSION` foi para 2. A abertura seguinte não usa o cache antigo.
- A leitura usa `O_NOATIME`, para não mudar o horário de acesso dos arquivos (sem ela quando o kernel recusa por não ser o dono).
- Walk de `/` (release, cache quente, 3 rodadas): 2,24 / 2,27 / 2,42 s com leitura e 2,10 / 2,15 / 2,62 s com `--no-sniff`. A diferença fica dentro do ruído.
- Nesta máquina, nenhum arquivo foi reclassificado: os que não têm extensão são binários, scripts e afins.
- Prova real: um JPEG de `/usr/share/backgrounds` copiado como `~/teste-sniff-slkr/foto-sem-extensao` apareceu em Fotos. O total escaneado do `$HOME` ficou a 12 KB do `du -sxB1 ~`, porque arquivos mudam entre as duas leituras.
- Workspace: 54 testes passam (5 novos), clippy limpo, `npm run check` sem erros.

---

## Etapa 9 — Docker rootless (backlog 7)
No modo rootless, os dados ficam em `~/.local/share/docker`, dentro do `$HOME`. Hoje o walk os conta como **Outros** (regra 4, pasta oculta) e os arquivos poderiam aparecer como "podem ir para a Lixeira". Parte da árvore pertence a subuids mapeados, e o walk não consegue ler. Esse pedaço cai no "sem acesso", enquanto o `docker system df` (do contexto padrão) não informa nada sobre ele.
- [x] `classify.rs`: `~/.local/share/docker` (e `~/.local/share/containers`, do Podman) entram na **regra 1**: Apps/Sistema, `is_app_path` verdadeiro e nunca vão para a Lixeira. Teste junto com os casos da Etapa 2.
- [x] `walk.rs`: somar à parte os bytes lidos sob esse prefixo (`rootless_scanned_bytes`). Eles continuam dentro de Apps/Sistema, mas o detalhe passa a mostrá-los como Docker.
- [x] `docker.rs`: descobrir os daemons acessíveis sem root:
  - rootful pelo socket padrão;
  - rootless por `unix://$XDG_RUNTIME_DIR/docker.sock`, quando existir, via `docker --host … system df` com host fixo montado no Rust, nunca vindo da UI.
  - `docker_status` devolve `DockerStatus { rootful, rootless }`, com `Option<DockerUsage>` em cada um. Os dois são consultados em paralelo e cada um tem seu timeout. Na implementação, o rootful também ganhou socket fixo (`/var/run/docker.sock`), para não ser confundido com o rootless depois de um `docker context use rootless`.
- [x] `model.rs::apply_docker`:
  - o rootful continua descontado do restante (limitado a ele);
  - do rootless, só a parte **não lida** pelo walk (`total − rootless_scanned_bytes`) sai do restante, também limitada, para não contar duas vezes;
  - a barra continua em 100%. Atualizar os testes de `apply_docker`/`set_disk`.
- [x] `docker_prune`: `DockerTarget` ganha o modo (`rootful`/`rootless`) e os argumentos continuam fixos (`--host` montado no Rust). Volumes continuam sem botão (D18).
- [x] UI: um card por daemon ("Docker" e "Docker rootless"), cada um com os seus botões. Sem nenhum daemon, não aparece nenhum, como hoje.
- [x] `cache::VERSION` incrementado, porque o formato de `DockerUsage` muda. `api.ts` espelha os tipos novos.
- [x] Testes com Docker falso: só rootful, só rootless, os dois juntos, e rootless com parte já lida pelo walk (sem contar duas vezes). Um arquivo sob `~/.local/share/docker` é recusado pelo `move_to_trash`.
- [x] Docs: ARCHITECTURE §5 (regra 1) e §6.1; DECISIONS D38 (por que socket fixo e não `docker context`, e a regra de não contar duas vezes).

**Pronto quando:**
- Com Docker rootless instalado (`dockerd-rootless-setuptool.sh install`), o card mostra o uso rootless, `~/.local/share/docker` não aparece mais em Outros e scan + Docker + sem acesso continua = `df`.
- Sem rootless, nada muda em relação à Etapa 7.

**Resultado (17/10/2026):**
- `ScanResult.docker` virou `DockerStatus { rootful, rootless }` e ganhou `rootless_scanned_bytes`. O evento `docker` traz `docker` no lugar de `usage`, `docker_prune` recebe `mode` e o `cache::VERSION` foi para 3. O `Done` passou a levar o resultado num `Box` (o clippy apontou o tamanho do enum), e o JSON não muda.
- `docker_prune` recusa um modo sem daemon no último resultado (`NoDocker`) e relê só o daemon limpo.
- Sem rootless, nada mudou. O CLI em `/` mostra o mesmo Docker de antes pelo socket fixo (75,80 GB, 75,50 GB liberáveis), e o walk continua em cerca de 2–3 s.
- 9 testes novos:
  - regra 1 para `~/.local/share/{docker,containers}`;
  - `rootless_scanned_bytes` no walk;
  - `apply_docker` com só rootless, com os dois daemons e com tudo já lido;
  - sockets fixos e `DockerStatus`;
  - ciclo completo da ponte com os dois daemons;
  - prune por modo, recusa sem daemon e falha sem mexer no resultado;
  - arquivo do rootless recusado pelo `move_to_trash`.
- Workspace: 62 testes passam, clippy limpo, `npm run check` sem erros.
- ⏳ **Falta a conferência com um rootless de verdade**, que nesta máquina não está configurado (só as ferramentas estão instaladas). Ativar com `dockerd-rootless-setuptool.sh install`, rodar `target/release/scan /` e conferir a linha "docker rootless:", o "Docker rootless lido" e scan + Docker + sem acesso = `df`.

---

## Etapa 10 — Cards de sugestão (backlog 2)
A tela principal ganha, abaixo da legenda, cards com o que dá para limpar com segurança: "Downloads antigos", "Caches" e "10 maiores arquivos".
- [ ] **Pasta de Downloads localizada:** ler `XDG_DOWNLOAD_DIR` de `~/.config/user-dirs.dirs` (`~/Downloads`, `~/Transferências`…), com fallback para `~/Downloads`. Fica no `Classifier`/`ScanOptions`, sem I/O no walk.
- [ ] `walk.rs` agrega, sem mudar as categorias:
  - **Downloads antigos:** arquivos sob a pasta de Downloads com mtime há mais de 90 dias (`OLD_DOWNLOAD_DAYS`, relativo ao início do scan). Bytes, contagem e top-N próprio (heap como o das categorias).
  - **Caches:** bytes por subpasta de primeiro nível de `~/.cache` (`~/.cache/mozilla`, `~/.cache/pip`…), mais `~/.cache/thumbnails`. Só as 20 maiores vão para o resultado.
- [ ] `model.rs`: `ScanResult.suggestions: Suggestions { old_downloads: CategoryStats, caches: Vec<DirEntry { path, bytes }> }`. Incrementar `cache::VERSION`.
- [ ] `record_trashed`/`find_file` passam a olhar também `old_downloads.top`, para o arquivo sair dos dois lugares e a barra continuar certa. `clear_trash` não precisa mudar.
- [ ] **Ações:**
  - Downloads antigos: mesma `move_to_trash`. `contains_file`/`check_trashable` aceitam os arquivos de `old_downloads.top`, com a mesma validação (D33).
  - Caches: novo command `trash_cache_dir(path)`. Só aceita um `path` que esteja em `suggestions.caches`, com canônico igual e **filho direto** de `~/.cache`. Move a pasta inteira para a Lixeira (D17: nada de `rm`). Atualiza o resultado: tira os bytes de Outros e soma em `trash_bytes`.
  - 10 maiores: novo command `largest_files(n)`, com `n ≤ 50`, que une os top-N de todas as categorias e devolve `FileItem` com `trashable`, reaproveitando `category_files`.
  - Seguir os 4 passos de "Adicionar um command" do CLAUDE.md para cada command novo, mais os wrappers em `api.ts`.
- [ ] **UI:**
  - `SuggestionCard.svelte`: título, tamanho total e um resumo ("37 arquivos com mais de 90 dias").
  - Clicar abre uma lista no estilo do `CategoryDetail`, reaproveitando `FileRow`, a seleção múltipla e o rodapé "Mover para a Lixeira". Para os caches, cada linha é uma pasta com o botão "Mover para a Lixeira", que pede confirmação no `ConfirmDialog`.
  - Card sem nada a sugerir não aparece.
- [ ] Testes:
  - walk numa pasta temporária com mtimes ajustados (`filetime` só em dev-deps, ou `utimensat` via `rustix`): Downloads antigos conta só os velhos;
  - caches agregados por subpasta;
  - `trash_cache_dir` recusa `~/.cache` em si, um neto, um caminho fora da lista e um link simbólico trocado depois do scan;
  - `record_trashed` de um arquivo antigo de Downloads atualiza categoria e sugestão.
- [ ] Docs: ARCHITECTURE §3, §4, §7 e §8; DECISIONS D39 (90 dias, caches por subpasta movidos para a Lixeira em vez de apagados, "10 maiores" calculado dos top-N já existentes).

**Pronto quando:**
- Os três cards aparecem com valores que conferem com `find ~/Downloads -mtime +90 -type f` e `du -sB1 ~/.cache/*`.
- Mover um cache para a Lixeira faz ele aparecer no Nautilus (restaurável) e o card "Lixeira" crescer no mesmo valor.
- Walk de `/` continua < 5 s.

---

## Etapa 11 — Múltiplos discos e partições (backlog 6)
Hoje o app só escaneia `/` e não cruza filesystems (D07). Com `/home` numa partição separada, o `$HOME` inteiro fica de fora. HDs internos e pendrives também nunca aparecem.
- [ ] `disk.rs`: `list_disks() -> Vec<DiskInfo>` a partir de `/proc/self/mounts`:
  - só filesystems de dados (ext2/3/4, btrfs, xfs, f2fs, vfat, exfat, ntfs3, fuseblk);
  - fora `squashfs`/loop (snaps), `tmpfs`, `/boot` e `/boot/efi`;
  - o mesmo dispositivo montado em dois lugares (subvolumes btrfs, bind mounts) aparece uma vez;
  - `DiskInfo` ganha `label` (do `/dev/disk/by-label`, ou o nome do mount) e `removable` (`/sys/block/*/removable`).
- [ ] `classify.rs`: `Classifier::new(home, data_root)`. Num disco que não é o do sistema nem o do `$HOME`, `data_root` = ponto de montagem, e as regras 4–7 valem relativas a ele (pasta oculta = Outros, extensão = mídia). A regra 8 ("fora do `$HOME` = Apps/Sistema") vale só no disco do sistema. `.Trash-<uid>` na raiz do mount conta como Lixeira (regra 2).
- [ ] `walk.rs`: nada muda no walk em si; `ScanOptions.root` passa a ser o mount escolhido.
- [ ] **Docker:** aplicado só ao disco que contém a raiz de dados de cada daemon (`docker info --format '{{.DockerRootDir}}'`, ou `/var/lib/docker` e `~/.local/share/docker` da Etapa 9). Nos outros discos, `apply_docker(None)`.
- [ ] **Estado e cache:**
  - `AppState` guarda o último resultado **por mount** (`HashMap<PathBuf, ScanResult>`), e um scan por vez;
  - `generation` continua valendo para o Docker;
  - cache em `~/.cache/storage-lkr/disks/<hash-do-mount>.json` (o `last.json` atual é migrado ou ignorado), mais `selected` com o último disco escolhido;
  - incrementar `cache::VERSION`.
- [ ] **Commands:**
  - `list_disks`;
  - `disk_info(mount)`, `start_scan(mount, on_event)`, `last_scan(mount)` e `category_files(mount, cat)`;
  - as ações de limpeza recebem `mount`, mas o Rust só aceita um mount que esteja em `list_disks()` e só age sobre o resultado **daquele** mount.
  - Atualizar `build.rs`, capabilities e `api.ts`.
- [ ] **Validação (`check_trashable`):** sob o `$HOME` canônico **ou**, num disco de dados, sob o mount e com dono = uid atual. As demais regras de D33 continuam. O crate `trash` já move para `$topdir/.Trash-$uid` em outro filesystem; testar isso. `empty_trash` já lista todas as lixeiras do usuário.
- [ ] **UI:**
  - seletor no header (`<select>` ou menu) com nome, tamanho e ícone de removível;
  - trocar de disco mostra o resultado em cache daquele disco na hora e reescaneia em segundo plano (D35);
  - os cards de sugestão (Etapa 10) só aparecem no disco que contém o `$HOME`;
  - disco removido durante o scan → `error` com mensagem clara, e o seletor recarrega.
- [ ] `bin/scan`: já aceita uma raiz. Adicionar `--list-disks`.
- [ ] Testes:
  - parser de `/proc/self/mounts` com uma amostra fixa (snap, tmpfs, btrfs duplicado, `/home` separado, pendrive vfat com espaço no nome);
  - classificação com `data_root` (`/media/u/HD/Fotos/a.jpg` vai para Fotos, `/media/u/HD/.Trash-1000/files/x` para a Lixeira);
  - estado com dois mounts: resultados e Docker não se misturam, e uma ação num mount fora da lista é recusada.
- [ ] Docs: ARCHITECTURE §1, §4, §5, §6, §7, §8 e §9; DECISIONS D40 (um disco por vez em vez de somar todos, por que `data_root`, e a regra de dono nos discos externos), que atualiza D09.

**Pronto quando:**
- O seletor lista `/`, um `/home` separado (se houver) e um pendrive conectado.
- Para cada disco, scan + (Docker) + sem acesso = `df -B1 <mount>`.
- Uma foto num pendrive aparece em Fotos, vai para a Lixeira e pode ser restaurada pelo Nautilus.
- Esvaziar a Lixeira libera o espaço no pendrive.
- Trocar de disco é instantâneo com cache.

---

## Backlog (pós-MVP)
1. Detalhar Apps por Snap, Flatpak e APT, com tamanho por app.
2. Cards de sugestão: "Downloads antigos", "Caches", "10 maiores arquivos". → **Etapa 10**
3. Atualização incremental (inotify ou fanotify).
4. Modo privilegiado via `pkexec` para escanear o que hoje é "sem acesso".
5. Detecção por magic bytes para arquivos sem extensão. → **Etapa 8** ✅
6. Suporte a múltiplos discos e partições (seletor no header). → **Etapa 11**
7. Docker rootless. → **Etapa 9** ✅
