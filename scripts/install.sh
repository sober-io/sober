#!/usr/bin/env bash
set -euo pipefail

# =============================================================================
# Sober — Install / Upgrade / Uninstall Script
# =============================================================================

SOBER_USER="${SOBER_USER:-sober}"
SOBER_VERSION="${SOBER_VERSION:-latest}"
INSTALL_DIR="/opt/sober"
CONFIG_DIR="/etc/sober"
SYSTEMD_DIR="/etc/systemd/system"
GITHUB_REPO="harrisiirak/s-ber"
NONINTERACTIVE="${NONINTERACTIVE:-0}"
UNINSTALL="${UNINSTALL:-0}"
EXTRACT_DIR=""

# -- Helpers ------------------------------------------------------------------

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

info()  { printf "${GREEN}[INFO]${NC}  %s\n" "$*"; }
warn()  { printf "${YELLOW}[WARN]${NC}  %s\n" "$*"; }
die()   { printf "${RED}[ERROR]${NC} %s\n" "$*" >&2; exit 1; }

fetch() {
    local url="$1"
    shift
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$url" "$@"
    elif command -v wget >/dev/null 2>&1; then
        # Map curl flags to wget
        local output=""
        while [ $# -gt 0 ]; do
            case "$1" in
                -o) output="$2"; shift 2 ;;
                *)  shift ;;
            esac
        done
        if [ -n "$output" ]; then
            wget -qO "$output" "$url"
        else
            wget -qO- "$url"
        fi
    else
        die "Neither curl nor wget found"
    fi
}

prompt_required() {
    local var_name="$1"
    local description="$2"
    local default="$3"

    # Check if already set via environment or CLI flag
    local current_value="${!var_name:-}"
    if [ -n "$current_value" ]; then
        return
    fi

    if [ "$NONINTERACTIVE" = "1" ]; then
        if [ -n "$default" ]; then
            eval "$var_name='$default'"
        else
            die "Required value $var_name not provided (non-interactive mode)"
        fi
        return
    fi

    local prompt_text="$description"
    if [ -n "$default" ]; then
        prompt_text="$prompt_text [$default]"
    fi
    printf "%s: " "$prompt_text"
    read -r value
    if [ -z "$value" ] && [ -n "$default" ]; then
        value="$default"
    fi
    if [ -z "$value" ]; then
        die "$var_name is required"
    fi
    eval "$var_name='$value'"
}

validate_database() {
    local url="$1"
    if command -v pg_isready >/dev/null 2>&1; then
        if pg_isready -d "$url" >/dev/null 2>&1; then
            info "Database connection verified"
        else
            warn "Could not connect to database. Ensure PostgreSQL is running."
        fi
    elif command -v psql >/dev/null 2>&1; then
        if psql "$url" -c "SELECT 1" >/dev/null 2>&1; then
            info "Database connection verified"
        else
            warn "Could not connect to database. Ensure PostgreSQL is running."
        fi
    else
        warn "Neither pg_isready nor psql found — skipping database validation"
    fi
}

# -- Argument Parsing ---------------------------------------------------------

usage() {
    cat <<'USAGE'
Usage: install.sh [OPTIONS]

Options:
  --user=<name>           Run services as this user (default: sober)
  --version=<tag>         Install specific version (default: latest)
  --yes                   Non-interactive mode
  --uninstall             Remove binaries and services, preserve data
  --database-url=<url>    Set DATABASE_URL
  --llm-base-url=<url>    Set LLM_BASE_URL
  --llm-api-key=<key>     Set LLM_API_KEY
  --llm-model=<model>     Set LLM_MODEL
  --help                  Show this help message
USAGE
}

while [ $# -gt 0 ]; do
    case "$1" in
        --user=*)          SOBER_USER="${1#*=}" ;;
        --version=*)       SOBER_VERSION="${1#*=}" ;;
        --yes)             NONINTERACTIVE=1 ;;
        --uninstall)       UNINSTALL=1 ;;
        --database-url=*)  DATABASE_URL="${1#*=}" ;;
        --llm-base-url=*)  LLM_BASE_URL="${1#*=}" ;;
        --llm-api-key=*)   LLM_API_KEY="${1#*=}" ;;
        --llm-model=*)     LLM_MODEL="${1#*=}" ;;
        --help|-h)         usage; exit 0 ;;
        *)                 die "Unknown option: $1" ;;
    esac
    shift
done

# -- Mode Detection -----------------------------------------------------------

detect_mode() {
    if [ "$UNINSTALL" = "1" ]; then
        echo "uninstall"
    elif [ -x "$INSTALL_DIR/bin/sober-api" ]; then
        echo "upgrade"
    else
        echo "install"
    fi
}

# -- Prerequisites ------------------------------------------------------------

check_prerequisites() {
    command -v systemctl >/dev/null 2>&1 || die "systemctl not found — systemd is required"
    command -v curl >/dev/null 2>&1 || command -v wget >/dev/null 2>&1 || die "curl or wget required"

    ARCH=$(uname -m)
    case "$ARCH" in
        x86_64)  TARGET="x86_64-unknown-linux-gnu" ;;
        aarch64) TARGET="aarch64-unknown-linux-gnu" ;;
        *)       die "Unsupported architecture: $ARCH" ;;
    esac

    [ "$(uname -s)" = "Linux" ] || die "Only Linux is supported for bare-metal install"
    [ "$(id -u)" -eq 0 ] || die "install.sh must be run as root"
}

# -- User Management ----------------------------------------------------------

ensure_user() {
    if [ "$SOBER_USER" = "root" ]; then
        warn "Running as root is not recommended. Consider a dedicated service user."
        return
    fi

    if id "$SOBER_USER" >/dev/null 2>&1; then
        info "User '$SOBER_USER' already exists"
    else
        info "Creating system user '$SOBER_USER'"
        useradd --system --no-create-home \
            --home-dir "$INSTALL_DIR/data" \
            --shell /usr/sbin/nologin \
            "$SOBER_USER"
    fi
}

# -- Directory Setup ----------------------------------------------------------

create_directories() {
    mkdir -p "$INSTALL_DIR/bin"
    mkdir -p "$INSTALL_DIR/data/workspaces"
    mkdir -p "$INSTALL_DIR/data/blobs"
    mkdir -p "$INSTALL_DIR/data/keys"
    mkdir -p "$CONFIG_DIR"

    chown -R "$SOBER_USER:$SOBER_USER" "$INSTALL_DIR"
    chown -R "$SOBER_USER:$SOBER_USER" "$CONFIG_DIR"
}

# -- Download & Extract -------------------------------------------------------

download_and_extract() {
    if [ "$SOBER_VERSION" = "latest" ]; then
        SOBER_VERSION=$(fetch "https://api.github.com/repos/$GITHUB_REPO/releases/latest" \
            | grep '"tag_name"' | cut -d'"' -f4)
        [ -n "$SOBER_VERSION" ] || die "Could not determine latest version"
    fi

    local archive="sober-${SOBER_VERSION}-${TARGET}.tar.gz"
    local url="https://github.com/$GITHUB_REPO/releases/download/${SOBER_VERSION}/${archive}"
    local checksum_url="${url}.sha256"

    info "Downloading Sober $SOBER_VERSION for $TARGET"
    fetch "$url" -o "/tmp/$archive"
    fetch "$checksum_url" -o "/tmp/${archive}.sha256"

    info "Verifying checksum"
    (cd /tmp && sha256sum -c "${archive}.sha256") || die "Checksum verification failed"

    EXTRACT_DIR=$(mktemp -d)
    tar -xzf "/tmp/$archive" -C "$EXTRACT_DIR"
    rm -f "/tmp/$archive" "/tmp/${archive}.sha256"

    info "Installing binaries to $INSTALL_DIR/bin/"
    cp "$EXTRACT_DIR/bin/"* "$INSTALL_DIR/bin/"
    chmod +x "$INSTALL_DIR/bin/"*

    # Symlinks for CLI tools
    ln -sf "$INSTALL_DIR/bin/sober" /usr/local/bin/sober
    ln -sf "$INSTALL_DIR/bin/soberctl" /usr/local/bin/soberctl
}

# -- Configuration ------------------------------------------------------------

write_default_config() {
    cat > "$CONFIG_DIR/config.toml" <<'TOML'
[server]
host = "0.0.0.0"
port = 3000

[storage]
data_dir = "/opt/sober/data"

[database]
max_connections = 10

[qdrant]
url = "http://localhost:6334"

[logging]
level = "info"
TOML
}

collect_config() {
    # Skip if config already exists (upgrade)
    [ -f "$CONFIG_DIR/.env" ] && return

    prompt_required "DATABASE_URL" "PostgreSQL connection string" "postgres://sober:password@localhost/sober"
    prompt_required "LLM_BASE_URL" "LLM API base URL" ""
    prompt_required "LLM_API_KEY" "LLM API key" ""
    prompt_required "LLM_MODEL" "LLM model identifier" ""

    validate_database "$DATABASE_URL"

    # Write .env
    cat > "$CONFIG_DIR/.env" <<EOF
DATABASE_URL=$DATABASE_URL
LLM_BASE_URL=$LLM_BASE_URL
LLM_API_KEY=$LLM_API_KEY
LLM_MODEL=$LLM_MODEL
EOF
    chmod 0600 "$CONFIG_DIR/.env"
    chown "$SOBER_USER:$SOBER_USER" "$CONFIG_DIR/.env"

    # Write config.toml from bundled template
    cp "$EXTRACT_DIR/config/config.toml.example" "$CONFIG_DIR/config.toml" 2>/dev/null \
        || write_default_config
    chown "$SOBER_USER:$SOBER_USER" "$CONFIG_DIR/config.toml"
}

# -- Systemd ------------------------------------------------------------------

install_systemd() {
    local services="sober-agent sober-api sober-scheduler sober-web"

    for svc in $services; do
        sed "s/User=sober/User=$SOBER_USER/g; s/Group=sober/Group=$SOBER_USER/g" \
            "$EXTRACT_DIR/systemd/${svc}.service" > "$SYSTEMD_DIR/${svc}.service"
    done

    cp "$EXTRACT_DIR/systemd/sober.target" "$SYSTEMD_DIR/sober.target"

    systemctl daemon-reload
    systemctl enable sober.target
}

# -- Migrations ---------------------------------------------------------------

run_migrations() {
    info "Running database migrations"
    sudo -u "$SOBER_USER" "$INSTALL_DIR/bin/sober" migrate run \
        || die "Migration failed. Check DATABASE_URL in $CONFIG_DIR/.env"
    info "Migrations complete"
}

# -- Start & Verify -----------------------------------------------------------

start_and_verify() {
    info "Starting Sober services"
    systemctl start sober.target

    sleep 3

    local failed=0
    for svc in sober-agent sober-api sober-scheduler sober-web; do
        if systemctl is-active --quiet "$svc"; then
            info "$svc: running"
        else
            warn "$svc: failed to start (check: journalctl -u $svc)"
            failed=1
        fi
    done

    if [ "$failed" = "0" ]; then
        info "Sober is running. Access the web UI at http://localhost:3000"
    else
        warn "Some services failed to start. Check logs with: journalctl -u sober-*"
    fi
}

# -- Upgrade ------------------------------------------------------------------

do_upgrade() {
    local current_version
    current_version=$("$INSTALL_DIR/bin/sober-api" --version 2>/dev/null | awk '{print $2}') || true
    info "Current version: ${current_version:-unknown}"

    download_and_extract

    info "Stopping services"
    systemctl stop sober.target

    info "Binaries updated. Running migrations and restarting"
    install_systemd
    run_migrations
    start_and_verify
}

# -- Uninstall ----------------------------------------------------------------

do_uninstall() {
    info "Stopping and disabling Sober services"
    systemctl stop sober.target 2>/dev/null || true
    systemctl disable sober.target 2>/dev/null || true

    rm -f "$SYSTEMD_DIR"/sober-*.service "$SYSTEMD_DIR/sober.target"
    systemctl daemon-reload

    rm -rf "$INSTALL_DIR/bin"
    rm -f /usr/local/bin/sober /usr/local/bin/soberctl

    info "Sober binaries and services removed."
    info ""
    info "The following data was preserved:"
    [ -d "$CONFIG_DIR" ] && info "  Configuration: $CONFIG_DIR/"
    [ -d "$INSTALL_DIR/data" ] && info "  Data:          $INSTALL_DIR/data/"
    info ""
    info "To remove manually:"
    info "  rm -rf $CONFIG_DIR $INSTALL_DIR/data"
    [ "$SOBER_USER" != "root" ] && info "  userdel $SOBER_USER"
}

# -- Main ---------------------------------------------------------------------

cleanup() {
    [ -n "${EXTRACT_DIR:-}" ] && rm -rf "$EXTRACT_DIR"
}
trap cleanup EXIT

main() {
    check_prerequisites

    MODE=$(detect_mode)

    case "$MODE" in
        install)
            info "Fresh install of Sober"
            ensure_user
            create_directories
            download_and_extract
            collect_config
            install_systemd
            run_migrations
            start_and_verify
            ;;
        upgrade)
            info "Upgrading Sober"
            do_upgrade
            ;;
        uninstall)
            do_uninstall
            ;;
    esac
}

main "$@"
