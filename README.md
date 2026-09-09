# Screen Share (Ultra-Low Latency Native Desktop)

**Screen Share** é um projeto de controle remoto de desktop construído 100% em **Rust**, focado em entregar a menor latência de rede e processamento possível. Inspirado por sistemas de alto desempenho como Moonlight e RustDesk, este projeto descarta completamente o ecossistema de navegadores e WebRTC em favor de transporte via **Raw TCP** e decodificação ponta a ponta acelerada por hardware.

## 🚀 Como Funciona?

### Arquitetura Cliente-Servidor Nativa
- **Servidor (Linux Host):** Construído em Rust. Utiliza `kmsgrab` para capturar os quadros da tela diretamente da memória da placa de vídeo (DRM/KMS), sem passar pelo servidor X11 ou Wayland. Os quadros são codificados nativamente em **H.264 / HEVC** pela GPU usando `h264_vaapi` via `ffmpeg` com preset de baixa latência e GOP curto para jogos. O resultado bruto (Annex-B NAL Units) é injetado diretamente em um socket TCP na porta `5000`. Eventos de mouse e teclado são recebidos via TCP na porta `5001` e injetados no Kernel Linux usando dispositivos virtuais `uinput` (via biblioteca `evdev`).
- **Cliente (macOS Nativo):** Um aplicativo nativo de altíssima performance escrito em **Swift** com **SwiftUI**, **VideoToolbox** e **AVSampleBufferDisplayLayer**. Ele conecta no socket TCP de vídeo, decodifica os frames H.264/HEVC diretamente no hardware da Apple (Apple Silicon / Intel) e renderiza na tela com Zero-Copy ($< 1\text{ ms}$ de overhead e $\approx 1\%$ de CPU). O cliente inclui Launcher moderno em SwiftUI com histórico de servidores, seleção de codec e HUD de FPS/Latência em tempo real.

## ✨ Recursos Principais
- **Zero-Copy Pipeline Completo:** Captura direta na GPU do host e decodificação/renderização direta na GPU do Mac via VideoToolbox.
- **Hardware Encoding/Decoding:** Codificação VAAPI (AMD/Intel) no host e decodificação nativa por hardware no Mac para consumo mínimo de CPU e bateria.
- **Raw TCP Transport com Zero-Buffer:** Sem overhead de WebRTC/RTP. Descarte ativo de buffers acumulados para latência zero em jogos.
- **HUD Integrado:** Medidor de FPS e Latência RTT em tempo real com indicador LED.
- **Controle Total:** Mouse e teclado virtuais no nível de kernel do Linux (`uinput`), garantindo compatibilidade com jogos.

## 🛠️ Pré-requisitos

### No Servidor (A máquina Linux sendo controlada)
- SO: Linux com suporte a DRM/KMS.
- Hardware: Placa de vídeo compatível com VAAPI (Intel, AMD).
- Dependências de sistema:
  ```bash
  sudo apt install ffmpeg libva-drm2 libva-x11-2 libavcodec-extra
  ```

### No Cliente (macOS)
- SO: macOS 13.0 (Ventura) ou superior (Apple Silicon M1/M2/M3/M4 ou Intel Mac).
- Zero dependências externas (não requer Homebrew, SDL2 ou FFmpeg).

## 🏗️ Compilação e Uso

### 1. Servidor (Linux Host)

Na máquina Linux a ser controlada:

```bash
# Compilar o servidor
cargo build --release -p server

# Executar o servidor (padrão HEVC / 1080p, exige root para KMS e uinput)
sudo ./target/release/server

# Ou especificando resolução 2K / 4K:
sudo ./target/release/server --res 2k

# Ou com codec H.264 (legado/fallback):
sudo ./target/release/server --codec h264
```

---

### 2. Cliente (macOS Nativo em Swift)

No seu Mac:

#### Compilar o Cliente:
```bash
cd client
swift build -c release
```

#### Executar:
- **Modo Gráfico (Launcher Interativo):**
  ```bash
  ./client/.build/release/ScreenShareClient
  ```
  *(Abre a interface nativa em SwiftUI com HEVC pré-selecionado por padrão, permitindo escolher o codec, resolução [1080p, 2K, 4K] e histórico)*

- **Modo Conexão Direta (CLI):**
  ```bash
  # Conexão direta com HEVC em 1080p (padrão do projeto)
  ./client/.build/release/ScreenShareClient 192.168.x.x

  # Conexão direta em 2K (1440p) ou 4K com HEVC
  ./client/.build/release/ScreenShareClient 192.168.x.x --res 2k
  ./client/.build/release/ScreenShareClient 192.168.x.x --res 4k

  # Conexão direta forçando H.264 (fallback)
  ./client/.build/release/ScreenShareClient 192.168.x.x --codec h264
  ```

---

*Para instruções passo a passo detalhadas, consulte o [TUTORIAL.md](./TUTORIAL.md).*
