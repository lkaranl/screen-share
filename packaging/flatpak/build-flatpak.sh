#!/usr/bin/env bash
set -euo pipefail

# Script automatizado para compilação e instalação do Flatpak do Screen Share Server
# Funciona em qualquer distribuição Linux (Fedora, Silverblue, Ubuntu, Arch, Debian, openSUSE)

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
MANIFEST="${ROOT_DIR}/packaging/flatpak/io.github.lkaranl.ScreenShareServer.yml"
BUILD_DIR="${ROOT_DIR}/packaging/flatpak/.build-dir"
REPO_DIR="${ROOT_DIR}/packaging/flatpak/.repo"
BUNDLE_FILE="${ROOT_DIR}/io.github.lkaranl.ScreenShareServer.flatpak"

cd "${ROOT_DIR}"

echo "=========================================================="
echo "📦 Compilação do Flatpak: io.github.lkaranl.ScreenShareServer"
echo "=========================================================="

# 1. Garante que os runtimes e SDKs necessários estejam instalados
echo "🔍 Verificando runtimes e SDKs do Flatpak (24.08)..."

flatpak remote-add --if-not-exists --user flathub https://dl.flathub.org/repo/flathub.flatpakrepo || true

flatpak install -y --noninteractive --user flathub \
    org.freedesktop.Platform//24.08 \
    org.freedesktop.Sdk//24.08 \
    org.freedesktop.Sdk.Extension.rust-stable//24.08 || true

# 2. Determina como invocar o flatpak-builder
BUILDER_CMD=""
if command -v flatpak-builder > /dev/null 2>&1; then
    BUILDER_CMD="flatpak-builder"
else
    echo "⬇️  flatpak-builder nativo não encontrado. Garantindo org.flatpak.Builder via Flatpak..."
    flatpak install -y --noninteractive --user flathub org.flatpak.Builder || true
    BUILDER_CMD="flatpak run org.flatpak.Builder"
fi

# 3. Compilação do Flatpak
echo "⚙️  Executando compilação do Flatpak com flatpak-builder..."
${BUILDER_CMD} \
    --force-clean \
    --user \
    --install \
    --repo="${REPO_DIR}" \
    "${BUILD_DIR}" \
    "${MANIFEST}"

# 4. Criação do arquivo de pacote (.flatpak bundle universal)
echo "📦 Gerando pacote universal (.flatpak bundle)..."
flatpak build-bundle "${REPO_DIR}" "${BUNDLE_FILE}" io.github.lkaranl.ScreenShareServer

echo "=========================================================="
echo "✅ Flatpak compilado e instalado com sucesso no seu usuário!"
echo "   App ID: io.github.lkaranl.ScreenShareServer"
echo "   Pacote distribuível gerado em:"
echo "   ${BUNDLE_FILE}"
echo ""
echo "Para executar em qualquer outra máquina Linux:"
echo "   1. Instale o pacote: flatpak install ${BUNDLE_FILE}"
echo "   2. Execute a regra udev do host: ./packaging/install-host-rules.sh"
echo "=========================================================="
