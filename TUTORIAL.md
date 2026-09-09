# Tutorial Rápido: Como rodar o RS-View

Siga este passo a passo direto ao ponto para conectar as máquinas.

---

## 1. Prepare as Máquinas (Dependências)

**No Linux (A máquina que vai ser controlada):**
Verifique se o `ffmpeg` e o `unclutter` (usado para ocultar o cursor remoto) estão instalados:
```bash
sudo apt update
sudo apt install ffmpeg unclutter libva-drm2 libva-x11-2 libavcodec-extra
```

**No seu Mac (A máquina que vai visualizar):**
- **Zero dependências externas!** O cliente Swift usa os aceleradores nativos do macOS (`VideoToolbox`, `CoreMedia`, `AVFoundation`, `Network.framework` e `SwiftUI`).

---

## 2. Compile os Componentes

### No Servidor (Linux):
```bash
cargo build --release -p server
```

### No Cliente (Mac):
```bash
cd client
swift build -c release
cd ..
```

---

## 3. Inicie o Servidor (No Linux)

O servidor precisa ser executado como Root (para conseguir capturar a placa de vídeo via `kmsgrab` e simular o teclado/mouse virtual via `uinput`).

Para rodar com o padrão do projeto (**HEVC / H.265**):
```bash
sudo ./target/release/server
```

Para rodar com resolução padrão **2K** ou **4K**:
```bash
sudo ./target/release/server --res 2k
```

Para rodar com o codec legado/fallback (**H.264**):
```bash
sudo ./target/release/server --codec h264
```

### Método Alternativo: Como Serviço de Segundo Plano (Systemd Daemon) - Recomendado

Para não precisar manter um terminal aberto e fazer o servidor iniciar automaticamente no boot do sistema:

```bash
./server/install_service.sh
```

Esse script compila o projeto em modo release, instala em `/usr/local/bin/screen-share-server` e configura a inicialização automática via Systemd.

- **Ver status:** `sudo systemctl status screen-share`
- **Acompanhar logs:** `journalctl -u screen-share -f`
- **Reiniciar:** `sudo systemctl restart screen-share`
- **Parar:** `sudo systemctl stop screen-share`
- **Desinstalar:** `./server/uninstall_service.sh`

---

## 4. Conecte o Cliente (No Mac)

### Método 1: Usando a Interface Gráfica (Recomendado)
Para abrir o Launcher gráfico moderno em SwiftUI (onde você pode salvar o IP, selecionar codec e resolução [1080p, 2K, 4K]):
```bash
./client/.build/release/ScreenShareClient
```
Uma janela gráfica em Dark Glassmorphism se abrirá com **HEVC** já selecionado por padrão. Basta inserir o IP do host Linux, escolher a resolução desejada (1080p, 2K ou 4K) e clicar em **Conectar**.

### Método 2: Conexão Direta (Via CLI)
Se preferir pular a interface gráfica e iniciar a conexão diretamente pelo terminal:

Para conectar no modo padrão (**HEVC em 1080p**):
```bash
./client/.build/release/ScreenShareClient 192.168.x.x
```

Para conectar usando **HEVC** em **2K (1440p)** ou **4K (2160p)**:
```bash
./client/.build/release/ScreenShareClient 192.168.x.x --res 2k
./client/.build/release/ScreenShareClient 192.168.x.x --res 4k
```

Para forçar o codec legado (**H.264**):
```bash
./client/.build/release/ScreenShareClient 192.168.x.x --codec h264
```
*(Substitua `192.168.x.x` pelo IP anotado)*

**Pronto!** O RS-View abrirá a janela de streaming nativa com decodificação por hardware direta na GPU, latência sub-milissegundo e HUD de FPS/ms em tempo real.