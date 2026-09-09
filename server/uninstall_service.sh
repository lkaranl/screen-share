#!/usr/bin/env bash
set -euo pipefail

# ─────────────────────────────────────────────────────────────────────────────
# 1. Verificações de Ambiente
# ─────────────────────────────────────────────────────────────────────────────

if [ "$(uname -s)" != "Linux" ]; then
    echo "❌ Erro: Este script de desinstalação deve ser executado no Linux."
    exit 1
fi

if ! command -v systemctl &>/dev/null; then
    echo "❌ Erro: O comando 'systemctl' não foi encontrado."
    exit 1
fi

if ! command -v sudo &>/dev/null && [ "$(id -u)" -ne 0 ]; then
    echo "❌ Erro: 'sudo' não encontrado e o script não está sendo executado como root."
    exit 1
fi

SERVICE_NAME="screen-share.service"
SYSTEMD_FILE="/etc/systemd/system/$SERVICE_NAME"
BINARY_FILE="/usr/local/bin/screen-share-server"

# Verifica se o serviço realmente está instalado
if [ ! -f "$SYSTEMD_FILE" ] && [ ! -f "$BINARY_FILE" ]; then
    echo "ℹ️  O serviço screen-share não parece estar instalado neste sistema."
    exit 0
fi

# ─────────────────────────────────────────────────────────────────────────────
# 2. Parar e Desabilitar Serviço
# ─────────────────────────────────────────────────────────────────────────────

echo "🛑 Verificando e interrompendo o serviço $SERVICE_NAME..."
if systemctl is-active --quiet "$SERVICE_NAME" 2>/dev/null; then
    echo "   Parando serviço ativo..."
    sudo systemctl stop "$SERVICE_NAME"
fi

if systemctl is-enabled --quiet "$SERVICE_NAME" 2>/dev/null; then
    echo "   Desabilitando inicialização automática no boot..."
    sudo systemctl disable "$SERVICE_NAME"
fi

# ─────────────────────────────────────────────────────────────────────────────
# 3. Limpeza de Regras de Firewall (UFW / Firewalld)
# ─────────────────────────────────────────────────────────────────────────────

echo "🔥 Limpando regras de firewall das portas 5000 e 5001..."

if command -v ufw &>/dev/null; then
    if sudo ufw status 2>/dev/null | grep -q "Status: active"; then
        sudo ufw delete allow 5000/tcp 2>/dev/null || true
        sudo ufw delete allow 5000/udp 2>/dev/null || true
        sudo ufw delete allow 5001/tcp 2>/dev/null || true
        echo "   ✅ Regras removidas do UFW."
    fi
fi

if command -v firewall-cmd &>/dev/null; then
    if sudo firewall-cmd --state 2>/dev/null | grep -q "running"; then
        sudo firewall-cmd --remove-port=5000/tcp --remove-port=5000/udp --remove-port=5001/tcp --permanent 2>/dev/null || true
        sudo firewall-cmd --reload 2>/dev/null || true
        echo "   ✅ Regras removidas do Firewalld."
    fi
fi

# ─────────────────────────────────────────────────────────────────────────────
# 4. Remoção de Arquivos e Limpeza do Systemd
# ─────────────────────────────────────────────────────────────────────────────

echo "🗑️  Removendo arquivos do serviço..."
if [ -f "$SYSTEMD_FILE" ]; then
    sudo rm -f "$SYSTEMD_FILE"
    echo "   Removido: $SYSTEMD_FILE"
fi

if [ -f "$BINARY_FILE" ]; then
    sudo rm -f "$BINARY_FILE"
    echo "   Removido: $BINARY_FILE"
fi

echo "🔄 Recarregando configurações do systemd..."
sudo systemctl daemon-reload
sudo systemctl reset-failed "$SERVICE_NAME" 2>/dev/null || true

echo ""
echo "================================================================="
echo "  ✅ Serviço screen-share foi completamente desinstalado!"
echo "================================================================="
echo ""
