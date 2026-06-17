# Tutorial Rápido: Como rodar o Screen Share

Siga este passo a passo direto ao ponto para conectar as máquinas.

## 1. Prepare as Máquinas (Dependências)

**No Linux (A máquina que vai ser controlada):**
Verifique se o `ffmpeg` está instalado e tem suporte a VAAPI:
```bash
sudo apt update
sudo apt install ffmpeg
```

**No seu Mac (A máquina que vai visualizar):**
Você precisa do SDL2 e das bibliotecas C do FFmpeg para decodificar o vídeo:
```bash
brew install sdl2 ffmpeg
```

---

## 2. Compile o Projeto

Em qualquer máquina onde o código fonte estiver baixado, na raiz do projeto, rode:
```bash
cargo build --release --workspace
```
*Isso vai compilar tanto a pasta `server` quanto a pasta `client` ao mesmo tempo.*

---

## 3. Inicie o Servidor (No Linux)

O servidor precisa ser executado como Root (para conseguir capturar a placa de vídeo via `kmsgrab` e simular o teclado/mouse virtual):

```bash
cd screen-share
sudo ./target/release/server
```

> **Atenção:** O terminal exibirá uma mensagem informando que os servidores TCP subiram (Portas 5000 e 5001) e mostrará o **IP do Linux** na rede local. Anote este IP.

---

## 4. Conecte o Cliente (No Mac)

Abra o terminal no seu Mac, vá até a pasta do projeto (caso tenha o código lá) e execute o Cliente passando o IP do Linux que você anotou:

```bash
cd screen-share
cargo run --release -p client -- 192.168.x.x
```
*(Substitua `192.168.x.x` pelo IP anotado)*

**Pronto!** Uma janela do SDL2 abrirá instantaneamente mostrando o vídeo do seu Linux, e todos os seus cliques e teclas serão enviados automaticamente.
