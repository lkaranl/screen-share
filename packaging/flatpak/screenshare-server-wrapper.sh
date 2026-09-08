#!/usr/bin/env bash
set -e

# Wrapper de inicialização do Screen Share Server dentro do Flatpak

LOCAL_IP=$(ip route get 1.1.1.1 2>/dev/null | awk '{print $7; exit}' || hostname -I 2>/dev/null | awk '{print $1}' || echo "0.0.0.0")

echo "=========================================="
echo "🚀 Screen Share Server (Flatpak)"
echo "📡 IP Local detectado: ${LOCAL_IP}"
echo "🎥 Porta de Vídeo (UDP): 5000"
echo "🎮 Porta de Controle (TCP): 5001"
echo "=========================================="

# Verifica se o /dev/uinput tem permissão de escrita
if [ ! -w /dev/uinput ]; then
    echo "⚠️ AVISO: /dev/uinput não tem permissão de escrita para este usuário."
    echo "Para habilitar teclado e mouse virtuais sem sudo, execute o instalador de regras no host:"
    echo "  ./packaging/install-host-rules.sh"
    if command -v notify-send > /dev/null 2>&1; then
        notify-send -u critical -i io.github.lkaranl.ScreenShareServer \
            "Screen Share Server" \
            "Atenção: Permissão em /dev/uinput necessária para mouse e teclado. Execute ./packaging/install-host-rules.sh no sistema." || true
    fi
fi

if command -v notify-send > /dev/null 2>&1; then
    notify-send -i io.github.lkaranl.ScreenShareServer \
        "Screen Share Server Ativo" \
        "Pronto para conexões!\nIP: ${LOCAL_IP}\nPorta: 5000 (Vídeo) / 5001 (Controle)" || true
fi

# Inicia o executável Rust do servidor
exec /app/bin/server "$@"
