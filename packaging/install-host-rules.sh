#!/usr/bin/env bash
set -euo pipefail

# Script de instalação da regra udev no host Linux
# Funciona em qualquer distribuição (Ubuntu, Debian, Fedora, Arch, openSUSE, etc.)

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
RULES_FILE="${SCRIPT_DIR}/udev/85-screenshare-uinput.rules"
DEST_FILE="/etc/udev/rules.d/85-screenshare-uinput.rules"

echo "🔧 Instalando regra udev para /dev/uinput..."

if [ ! -f "${RULES_FILE}" ]; then
    echo "❌ Arquivo de regras ${RULES_FILE} não encontrado."
    exit 1
fi

sudo cp "${RULES_FILE}" "${DEST_FILE}"
sudo chmod 644 "${DEST_FILE}"

echo "🔄 Recarregando regras udev do sistema..."
sudo udevadm control --reload-rules
sudo udevadm trigger --property-match=DEVNAME=/dev/uinput || true

# Garante que o módulo uinput esteja carregado no kernel
sudo modprobe uinput || true

# Se o grupo input existir, garante que o usuário faça parte dele
if getent group input > /dev/null 2>&1; then
    TARGET_USER="${SUDO_USER:-$USER}"
    if ! id -nG "${TARGET_USER}" | grep -qw "input"; then
        echo "👤 Adicionando o usuário ${TARGET_USER} ao grupo 'input'..."
        sudo usermod -aG input "${TARGET_USER}"
    fi
fi

echo "✅ Regras udev instaladas com sucesso!"
echo "   Dispositivo /dev/uinput agora pode ser acessado sem sudo."
