# Tutorial Rápido: Como rodar o RS-View

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

Em qualquer máquina onde o código fonte estiver baixado, na raiz do projeto, rode para compilar ambos ao mesmo tempo:
```bash
cargo build --release --workspace
```

**Caso queira compilar separadamente (por exemplo, compilar só o servidor no Linux e só o cliente no Mac):**

Para compilar apenas o Servidor:
```bash
cargo build --release -p server
```

Para compilar apenas o Cliente:
```bash
cargo build --release -p client
```

---

## 3. Inicie o Servidor (No Linux)

O servidor precisa ser executado como Root (para conseguir capturar a placa de vídeo via `kmsgrab` e simular o teclado/mouse virtual). O codec padrão é o H.264, mas você pode usar o H.265 (HEVC) para maior qualidade usando a mesma largura de banda.

Para rodar com o padrão (H.264):
```bash
cd screen-share
sudo ./target/release/server
```

Para rodar com suporte a **H.265 / HEVC**:
```bash
cd screen-share
sudo ./target/release/server --codec hevc
```

> **Atenção:** O terminal exibirá uma mensagem informando que o servidor de controle (TCP) subiu na porta 5001. O vídeo será enviado por UDP na porta 5000 diretamente para o IP do cliente conectado. O terminal também exibirá o **IP do Linux** na rede local. Anote este IP.

---

## 4. Conecte o Cliente (No Mac)

Abra o terminal no seu Mac e vá até a pasta do projeto.

### Método 1: Usando a Interface Gráfica (Recomendado)
Para abrir o Launcher gráfico (onde você pode salvar o IP e selecionar itens do histórico):
```bash
./target/release/client
```
Uma janela gráfica se abrirá. Basta inserir o IP do host Linux e pressionar `Enter` ou clicar em **Conectar**.

### Método 2: Conexão Direta (Via CLI)
Se preferir pular a interface gráfica e iniciar a conexão diretamente pelo terminal:

Para conectar no modo padrão (H.264):
```bash
./target/release/client 192.168.x.x
```

Para conectar usando o codec **H.265 / HEVC**:
```bash
./target/release/client 192.168.x.x --codec hevc
```
*(Substitua `192.168.x.x` pelo IP anotado)*

**Pronto!** O RS-View abrirá a janela de visualização do seu Linux, permitindo controle completo com mouse, teclado e sincronização bidirecional de clipboard.

cargo run --release -p client -- 192.168.68.51 --codec hevc