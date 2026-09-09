#!/usr/bin/env bash
set -euo pipefail

# ─────────────────────────────────────────────────────────────────────────────
# 1. Verificações de Ambiente e Pré-requisitos
# ─────────────────────────────────────────────────────────────────────────────

# Checagem de Sistema Operacional
if [ "$(uname -s)" != "Linux" ]; then
    echo "❌ Erro: Este serviço só pode ser instalado em sistemas operacionais Linux."
    exit 1
fi

# Checagem do Systemd
if ! command -v systemctl &>/dev/null; then
    echo "❌ Erro: O comando 'systemctl' não foi encontrado."
    echo "   Este sistema não parece utilizar o systemd como sistema de inicialização (init)."
    exit 1
fi

# Checagem de permissão sudo/root
if ! command -v sudo &>/dev/null && [ "$(id -u)" -ne 0 ]; then
    echo "❌ Erro: 'sudo' não encontrado e o script não está sendo executado como root."
    exit 1
fi

# Detecção de Sistema Imutável (Fedora Silverblue, Kinoite, Bazzite, SteamOS)
IS_OSTREE=false
if [ -f "/run/ostree-booted" ]; then
    IS_OSTREE=true
    echo "🧊 Sistema imutável detectado (OSTree / Fedora Silverblue / Bazzite)."
fi

# Checagem da ferramenta de compilação (Rust/Cargo)
if ! command -v cargo &>/dev/null; then
    echo "❌ Erro: O compilador 'cargo' (Rust) não foi encontrado no PATH."
    echo "   Por favor, instale o Rust antes de prosseguir (ex: https://rustup.rs ou gerenciador de pacotes)."
    exit 1
fi

# Checagem do FFmpeg
if ! command -v ffmpeg &>/dev/null; then
    echo "❌ Erro: O binário 'ffmpeg' não foi encontrado no sistema."
    echo "   O FFmpeg é obrigatório para a captura de tela KMS/DRM e aceleração por hardware VAAPI."
    echo ""
    echo "   👉 Como instalar:"
    if [ "$IS_OSTREE" = true ]; then
        echo "      • Fedora Silverblue: rpm-ostree install ffmpeg (com repositório RPM Fusion ativado)"
    else
        echo "      • Arch Linux:        sudo pacman -S ffmpeg"
        echo "      • Fedora Workstation: sudo dnf install ffmpeg"
        echo "      • Ubuntu / Debian:     sudo apt install ffmpeg"
    fi
    exit 1
fi

# Checagem de encoders de hardware VAAPI no FFmpeg
FFMPEG_ENCODERS="$(ffmpeg -encoders 2>/dev/null || true)"
if echo "$FFMPEG_ENCODERS" | grep -E "hevc_vaapi|h264_vaapi" >/dev/null; then
    echo "✅ FFmpeg com suporte a VAAPI detectado (hevc_vaapi / h264_vaapi)."
else
    echo "⚠️  Aviso: O FFmpeg instalado não listou 'hevc_vaapi' ou 'h264_vaapi'."
    echo "   Recomenda-se uma compilação do FFmpeg com suporte a VAAPI para melhor performance."
fi

# Checagem de dispositivos DRM de vídeo
if [ ! -d "/dev/dri" ]; then
    echo "⚠️  Aviso: O diretório '/dev/dri' não foi encontrado."
    echo "   Certifique-se de que os drivers de aceleração gráfica DRM/KMS estão carregados no kernel."
else
    echo "✅ Dispositivos gráficos DRM (/dev/dri) encontrados."
fi

# Checagem e ativação do uinput para controle remoto
if [ ! -e "/dev/uinput" ]; then
    echo "ℹ️  '/dev/uinput' não encontrado. Tentando carregar o módulo do kernel (modprobe uinput)..."
    sudo modprobe uinput 2>/dev/null || true
    if [ ! -e "/dev/uinput" ]; then
        echo "⚠️  Aviso: '/dev/uinput' ainda não está acessível. A injeção de mouse/teclado virtual pode necessitar do módulo carregado no boot."
    else
        echo "✅ Módulo uinput carregado com sucesso."
    fi
else
    echo "✅ Dispositivo virtual de input (/dev/uinput) pronto."
fi

# ─────────────────────────────────────────────────────────────────────────────
# 2. Compilação do Binário
# ─────────────────────────────────────────────────────────────────────────────

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

echo ""
echo "🔨 Compilando o servidor em modo release (cargo build --release -p server)..."
cd "$REPO_ROOT"
cargo build --release -p server

BINARY_SOURCE="$REPO_ROOT/target/release/server"
BINARY_DEST="/usr/local/bin/screen-share-server"
SERVICE_FILE="$SCRIPT_DIR/screen-share.service"
SYSTEMD_DEST="/etc/systemd/system/screen-share.service"

if [ ! -f "$BINARY_SOURCE" ]; then
    echo "❌ Erro: Binário compilado não encontrado em: $BINARY_SOURCE"
    exit 1
fi

if [ ! -f "$SERVICE_FILE" ]; then
    echo "❌ Erro: Arquivo de serviço não encontrado em: $SERVICE_FILE"
    exit 1
fi

# ─────────────────────────────────────────────────────────────────────────────
# 3. Instalação e Contexto de Segurança (SELinux)
# ─────────────────────────────────────────────────────────────────────────────

echo "📦 Instalando binário em $BINARY_DEST..."
sudo mkdir -p /usr/local/bin
sudo install -m 755 "$BINARY_SOURCE" "$BINARY_DEST"

echo "⚙️  Configurando serviço systemd em $SYSTEMD_DEST..."
sudo mkdir -p /etc/systemd/system
sudo install -m 644 "$SERVICE_FILE" "$SYSTEMD_DEST"

# Aplica contexto SELinux se ativo (Essencial para Fedora / Silverblue)
if command -v getenforce &>/dev/null; then
    SELINUX_STATUS="$(getenforce 2>/dev/null || echo "Disabled")"
    if [ "$SELINUX_STATUS" != "Disabled" ]; then
        echo "🛡️  Aplicando contexto de segurança SELinux (restorecon)..."
        sudo restorecon -v "$BINARY_DEST" "$SYSTEMD_DEST" || true
    fi
fi

# ─────────────────────────────────────────────────────────────────────────────
# 4. Configuração Automática de Firewall (UFW / Firewalld)
# ─────────────────────────────────────────────────────────────────────────────

echo ""
echo "🔥 Verificando regras de firewall (Portas 5000 UDP/TCP e 5001 TCP)..."

FIREWALL_CONFIGURED=false

# Detecção e configuração do UFW (comum no Arch, Ubuntu, Debian)
if command -v ufw &>/dev/null; then
    if sudo ufw status 2>/dev/null | grep -q "Status: active"; then
        echo "   Detectado UFW ativo. Liberando portas do screen-share..."
        sudo ufw allow 5000/tcp comment 'Screen Share Handshake' >/dev/null
        sudo ufw allow 5000/udp comment 'Screen Share Video Stream' >/dev/null
        sudo ufw allow 5001/tcp comment 'Screen Share Control' >/dev/null
        echo "   ✅ Regras liberadas no UFW com sucesso!"
        FIREWALL_CONFIGURED=true
    fi
fi

# Detecção e configuração do Firewalld (padrão no Fedora / Silverblue / RHEL)
if command -v firewall-cmd &>/dev/null && [ "$FIREWALL_CONFIGURED" = false ]; then
    if sudo firewall-cmd --state 2>/dev/null | grep -q "running"; then
        echo "   Detectado Firewalld ativo. Liberando portas do screen-share..."
        sudo firewall-cmd --add-port=5000/tcp --add-port=5000/udp --add-port=5001/tcp --permanent >/dev/null
        sudo firewall-cmd --reload >/dev/null
        echo "   ✅ Regras liberadas no Firewalld com sucesso!"
        FIREWALL_CONFIGURED=true
    fi
fi

if [ "$FIREWALL_CONFIGURED" = false ]; then
    echo "   ℹ️  Nenhum firewall ativo detectado (UFW/Firewalld). Se você usar iptables/nftables, certifique-se de liberar:"
    echo "      • Porta 5000 (TCP e UDP) - Handshake e stream de vídeo"
    echo "      • Porta 5001 (TCP)       - Canal de controle (mouse, teclado, clipboard)"
fi

# ─────────────────────────────────────────────────────────────────────────────
# 5. Ativação e Verificação do Serviço Systemd
# ─────────────────────────────────────────────────────────────────────────────

echo ""
echo "🔄 Recarregando configurações do systemd..."
sudo systemctl daemon-reload

echo "🚀 Habilitando e iniciando o serviço screen-share..."
sudo systemctl enable --now screen-share.service

# Verificação do status inicial
sleep 1
if systemctl is-active --quiet screen-share.service; then
    echo ""
    echo "================================================================="
    echo "  🎉 Serviço screen-share instalado e ATIVO com sucesso!"
    echo "================================================================="
else
    echo ""
    echo "⚠️  O serviço foi instalado, mas pode estar inicializando ou com aviso."
fi

echo ""
echo "Comandos úteis:"
echo "  • Ver status em tempo real: sudo systemctl status screen-share"
echo "  • Acompanhar logs ao vivo:  journalctl -u screen-share -f"
echo "  • Reiniciar o serviço:      sudo systemctl restart screen-share"
echo "  • Parar o serviço:          sudo systemctl stop screen-share"
echo "  • Desinstalar:              ./server/uninstall_service.sh"
echo ""
