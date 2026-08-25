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

Para rodar com o codec padrão (**H.264**):
```bash
sudo ./target/release/server
```

Para rodar com suporte a **H.265 / HEVC**:
```bash
sudo ./target/release/server --codec hevc
```

> **Atenção:** O terminal exibirá uma mensagem informando que os servidores TCP subiram (Portas 5000 e 5001) e mostrará o **IP do Linux** na rede local. Anote este IP.

---

## 4. Conecte o Cliente (No Mac)

### Método 1: Usando a Interface Gráfica (Recomendado)
Para abrir o Launcher gráfico moderno em SwiftUI (onde você pode salvar o IP e selecionar itens do histórico):
```bash
./client/.build/release/ScreenShareClient
```
Uma janela gráfica em Dark Glassmorphism se abrirá. Basta inserir o IP do host Linux e pressionar `Enter` ou clicar em **Conectar**.

### Método 2: Conexão Direta (Via CLI)
Se preferir pular a interface gráfica e iniciar a conexão diretamente pelo terminal:

Para conectar no modo padrão (**H.264**):
```bash
./client/.build/release/ScreenShareClient 192.168.x.x
```

Para conectar usando o codec **H.265 / HEVC**:
```bash
./client/.build/release/ScreenShareClient 192.168.x.x --codec hevc
```
*(Substitua `192.168.x.x` pelo IP anotado)*

**Pronto!** O RS-View abrirá a janela de streaming nativa com decodificação por hardware direta na GPU, latência sub-milissegundo e HUD de FPS/ms em tempo real.