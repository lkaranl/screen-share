#!/bin/bash
set -e

# Se receber argumentos de linha de comando diretos, executa o modo solicitado
MODE=""
TARGET_HOST=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --server|-s)
            MODE="server"
            shift
            ;;
        --client|-c)
            MODE="client"
            shift
            if [[ $# -gt 0 && ! "$1" =~ ^- ]]; then
                TARGET_HOST="$1"
                shift
            fi
            ;;
        --both|-b)
            MODE="both"
            shift
            ;;
        --help|-h)
            echo "Screen Share (Linux) - Lançador Integrado"
            echo "Uso: screenshare-launcher [OPÇÃO]"
            echo ""
            echo "Opções:"
            echo "  --server, -s              Executa exclusivamente como Servidor (compartilha a tela)"
            echo "  --client, -c [IP]         Executa exclusivamente como Cliente (conecta ao IP remoto)"
            echo "  --both, -b                Executa Servidor e Cliente simultaneamente em 127.0.0.1"
            echo "  --help, -h                Exibe esta mensagem de ajuda"
            exit 0
            ;;
        *)
            if [[ -z "$MODE" && ! "$1" =~ ^- ]]; then
                # Se passou apenas um IP, assume modo cliente
                MODE="client"
                TARGET_HOST="$1"
            fi
            shift
            ;;
    esac
done

# Se nenhum modo foi passado via CLI, abre diálogo visual se houver interface gráfica
if [[ -z "$MODE" ]]; then
    if which zenity >/dev/null 2>&1 && [[ -n "$WAYLAND_DISPLAY" || -n "$DISPLAY" ]]; then
        CHOICE=$(zenity --list --radiolist \
            --title="Screen Share - Seleção de Modo" \
            --text="Como você deseja utilizar o Screen Share nesta máquina?" \
            --column="Seleção" --column="ID" --column="Modo" --column="Descrição" \
            TRUE "server" "🖥️  Servidor" "Compartilhar esta tela e aceitar conexões remotas" \
            FALSE "client" "💻  Cliente" "Conectar a outro computador para visualizar e controlar" \
            FALSE "both" "🔄  Ambos" "Iniciar Servidor + Cliente simultâneos (Teste local)" \
            --hide-column=2 \
            --width=620 --height=280 2>/dev/null) || exit 0

        MODE="$CHOICE"
    else
        echo "=========================================================="
        echo "           Screen Share - Seleção de Modo                 "
        echo "=========================================================="
        echo "1) 🖥️  Servidor (Compartilhar esta tela)"
        echo "2) 💻  Cliente (Conectar a outro computador)"
        echo "3) 🔄  Ambos (Servidor + Cliente local para testes)"
        echo "=========================================================="
        read -rp "Escolha uma opção (1-3) [1]: " OPT
        case "$OPT" in
            2) MODE="client" ;;
            3) MODE="both" ;;
            *) MODE="server" ;;
        esac
    fi
fi

# Ações por modo escolhido
case "$MODE" in
    server)
        echo "🖥️  Iniciando modo Servidor..."
        exec screenshare-server-wrapper
        ;;

    client)
        if [[ -z "$TARGET_HOST" ]]; then
            if which zenity >/dev/null 2>&1 && [[ -n "$WAYLAND_DISPLAY" || -n "$DISPLAY" ]]; then
                TARGET_HOST=$(zenity --entry \
                    --title="Conectar a Servidor Remoto" \
                    --text="Digite o endereço IP do computador servidor:" \
                    --entry-text="127.0.0.1" 2>/dev/null) || exit 0
            else
                read -rp "Digite o IP do servidor [127.0.0.1]: " TARGET_HOST
                TARGET_HOST="${TARGET_HOST:-127.0.0.1}"
            fi
        fi
        echo "💻 Conectando como Cliente em $TARGET_HOST..."
        exec client-linux --host "$TARGET_HOST"
        ;;

    both)
        echo "🔄 Iniciando modo Ambos (Servidor + Cliente local em 127.0.0.1)..."
        # Inicia o servidor em segundo plano
        screenshare-server-wrapper &
        SERVER_PID=$!

        # Função de encerramento limpo
        cleanup() {
            echo "🛑 Encerrando servidor em segundo plano (PID $SERVER_PID)..."
            kill "$SERVER_PID" 2>/dev/null || true
        }
        trap cleanup EXIT INT TERM

        # Aguarda 1 segundo para as portas 5000 e 5001 iniciarem
        sleep 1

        # Inicia o cliente conectando em loopback
        client-linux --host 127.0.0.1
        ;;

    *)
        echo "Opção inválida. Use --help para ver as opções disponíveis."
        exit 1
        ;;
esac
