# Guia de Publicação no Flathub

Este guia contém o passo a passo completo para submeter e disponibilizar o **Screen Share Server** (`io.github.lkaranl.ScreenShareServer`) na loja oficial do [Flathub](https://flathub.org).

---

## 1. O que já está pronto e validado

O pacote já foi preparado atendendo a todas as diretrizes rígidas do Flathub:

- **App ID Canônico:** `io.github.lkaranl.ScreenShareServer` (formato DNS reverso oficial vinculado à sua conta GitHub `lkaranl`).
- **Validação AppStream (`metainfo.xml`):** Passou com 100% de sucesso no `appstreamcli validate` e `flatpak-builder-lint`, contendo metadados de licença (MIT/CC0-1.0), descrição, releases, screenshots e categoria.
- **Suite Completa de Ícones:** Resoluções padrão hicolor (16x16, 32x32, 48x48, 64x64, 128x128, 256x256, 512x512) e vetor escalável SVG instalados nas pastas corretas.
- **Build 100% Offline (Obrigatório no Flathub):** Os servidores de build do Flathub não possuem acesso à internet durante a compilação. Geramos o arquivo `cargo-sources.json` com todas as dependências Rust pré-indexadas e com hashes SHA-256 verificadas.
- **Acesso a Hardware Seguro:** O manifesto declara permissão `--device=all` (necessária para codificação VAAPI `/dev/dri` e injeção de input `/dev/uinput`), além de sockets Wayland/X11 e PipeWire.

---

## 2. Passo a Passo para Publicar

### Passo 1: Fazer Commit e Push das alterações no GitHub

Antes de abrir a solicitação no Flathub, o código com os arquivos de packaging deve estar no seu repositório oficial:

```bash
git add packaging/
git commit -m "feat(packaging): adicionar empacotamento flatpak, ícones e manifesto flathub"
git push origin main
```

### Passo 2: Criar a Tag de Release `v0.1.0`

O manifesto do Flathub aponta para a tag `v0.1.0` do repositório `https://github.com/lkaranl/screen-share.git`.

Crie a tag no repositório:
```bash
git tag v0.1.0
git push origin v0.1.0
```

*(Opcional: Vá até a aba "Releases" no GitHub e crie a Release `v0.1.0` a partir dessa tag).*

---

### Passo 3: Fazer o Fork do Flathub

1. Acesse o repositório oficial de submissões do Flathub: [github.com/flathub/flathub](https://github.com/flathub/flathub).
2. Clique no botão **Fork** (canto superior direito) para criar uma cópia na sua conta GitHub (`lkaranl/flathub`).
3. Clone o seu fork na sua máquina:
   ```bash
   git clone git@github.com:lkaranl/flathub.git
   cd flathub
   ```

---

### Passo 4: Criar uma Branch e Copiar os Arquivos de Manifesto

Crie uma branch com o ID exato da aplicação:

```bash
git checkout -b io.github.lkaranl.ScreenShareServer
```

Copie os dois arquivos preparados da pasta `packaging/flatpak/flathub/` para a raiz do seu clone do Flathub:
- `io.github.lkaranl.ScreenShareServer.yml`
- `cargo-sources.json`

Faça o commit e o push para o seu fork:

```bash
git add io.github.lkaranl.ScreenShareServer.yml cargo-sources.json
git commit -m "Add io.github.lkaranl.ScreenShareServer"
git push -u origin io.github.lkaranl.ScreenShareServer
```

---

### Passo 5: Abrir o Pull Request no Flathub

1. Acesse o seu fork no GitHub: `https://github.com/lkaranl/flathub`.
2. O GitHub exibirá uma barra amarela sugerindo abrir um **Pull Request**. Clique em **Compare & pull request**.
3. Certifique-se de que:
   - **Base repository:** `flathub/flathub` (branch `new-pr`).
   - **Head repository:** `lkaranl/flathub` (branch `io.github.lkaranl.ScreenShareServer`).
4. Preencha o título: `Add io.github.lkaranl.ScreenShareServer`.
5. No corpo do Pull Request, marque as caixas de verificação padrão do template do Flathub.
   - **Nota sobre permissões:** Se o bot ou revisor perguntar sobre `--device=all`, informe que o aplicativo é um servidor de streaming de tela com aceleração de vídeo VAAPI por hardware (`/dev/dri`) e criação de teclado/mouse virtuais de baixa latência via `/dev/uinput` (o mesmo padrão usado pelo Sunshine e Weylus no Flathub).
6. Envie o Pull Request!

---

### Passo 6: Validação Automática e Aprovação

- Assim que você abrir o PR, o **Flathubbot** executará:
  1. Verificação de propriedade do App ID (validada automaticamente porque o PR partiu de `@lkaranl`).
  2. Compilação de teste em arquiteturas `x86_64` e `aarch64`.
  3. Linter automático (`flatpak-builder-lint`).
- Se houver algum comentário dos mantenedores do Flathub, faça os ajustes solicitados e dê `git push` na mesma branch.
- Uma vez aprovado e realizado o **Merge**, o Flathub criará automaticamente o repositório oficial:
  `https://github.com/flathub/io.github.lkaranl.ScreenShareServer`
- Em até algumas horas após o primeiro build de produção, seu aplicativo estará listado publicamente em:
  `https://flathub.org/apps/io.github.lkaranl.ScreenShareServer`
