# Screen Share (Ultra-Low Latency Native Desktop)

**Screen Share** é um projeto de controle remoto de desktop construído 100% em **Rust**, focado em entregar a menor latência de rede e processamento possível. Inspirado por sistemas de alto desempenho como Moonlight e RustDesk, este projeto descarta completamente o ecossistema de navegadores e WebRTC em favor de transporte via **Raw TCP** e decodificação ponta a ponta acelerada por hardware.

## 🚀 Como Funciona?

### Arquitetura Cliente-Servidor Nativa
- **Servidor (Linux Host):** Utiliza `kmsgrab` para capturar os quadros da tela diretamente da memória da placa de vídeo (DRM/KMS), sem passar pelo servidor X11 ou Wayland. Os quadros são codificados nativamente em **H.264** pela GPU usando `h264_vaapi` via `ffmpeg`. O resultado bruto (Annex-B NAL Units) é injetado diretamente em um socket TCP na porta `5000`. Eventos de mouse e teclado são recebidos via TCP na porta `5001` e injetados no Kernel Linux usando dispositivos virtuais `uinput` (via biblioteca `evdev`).
- **Cliente (Viewer):** Um aplicativo nativo de alta performance escrito em Rust. Ele conecta no socket de vídeo, decodifica os bytes puros de H.264 usando a API C do FFmpeg (`ffmpeg-next`), e renderiza os quadros diretamente em uma textura utilizando **SDL2**. O SDL2 também captura instantaneamente interações do usuário (teclado, mouse) e as envia como comandos JSON para o servidor.

## ✨ Recursos Principais
- **Zero-Copy Capture:** Captura de tela direto da placa de vídeo no Linux via KMS.
- **Hardware Encoding/Decoding:** Codificação VAAPI no host e decodificação nativa no cliente para consumo mínimo de CPU.
- **Raw TCP Transport:** Sem overhead de protocolos P2P, ICE, DTLS ou RTP. Dados diretos da GPU para o socket de rede.
- **Controle Total:** Mouse e teclado virtuais no nível de kernel do Linux (`uinput`), garantindo compatibilidade com jogos e interfaces gráficas que bloqueiam simulações de input de alto nível.

## 🛠️ Pré-requisitos

### No Servidor (A máquina Linux sendo controlada)
- SO: Linux com suporte a DRM/KMS.
- Hardware: Placa de vídeo compatível com VAAPI (Intel, AMD).
- Dependências de sistema:
  ```bash
  sudo apt install ffmpeg libva-drm2 libva-x11-2 libavcodec-extra
  ```
- O usuário executando o servidor precisa ter permissões de superusuário (`root` ou uso do `sudo`) devido às exigências do `kmsgrab` e do `uinput`.

### No Cliente (A máquina acessando remotamente, ex: Mac)
- SO: macOS, Windows ou Linux.
- Dependências de sistema (Exemplo para macOS com Homebrew):
  ```bash
  brew install sdl2 ffmpeg
  ```

## 🏗️ Compilação e Execução

O projeto está estruturado em um Workspace Cargo. Para compilar todos os módulos de uma vez:

```bash
cargo build --release --workspace
```

*Para instruções simplificadas de execução, consulte o [TUTORIAL.md](./TUTORIAL.md).*
