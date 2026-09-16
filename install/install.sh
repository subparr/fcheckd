#!/bin/bash
#SPDX-License-Identifier: GPL-3.0-only
#Copyright (C) 2026 subparr <subparr@tuta.io>

set -euo pipefail

BIN_NAME="fcheckd"
BIN_DEST="/usr/local/bin/${BIN_NAME}"
CONFIG_DIR="/etc/${BIN_NAME}"
HOME_CONFIG_DIR="${HOME}/.config/${BIN_NAME}"
CONFIG_EXAMPLE="install/etc/config.example.toml"
CONFIG_DEST="${CONFIG_DIR}/config.toml"
HOME_CONFIG_DEST="${HOME_CONFIG_DIR}/config.toml"
SYSTEMD_UNIT_SRC="install/initservices/${BIN_NAME}.service"
SYSTEMD_UNIT_DEST="/usr/lib/systemd/system/${BIN_NAME}.service"
HOME_SYSTEMD_UNIT_DEST="${HOME}/.config/systemd/user/${BIN_NAME}.service"
OPENRC_SCRIPT_SRC="install/initservices/${BIN_NAME}"
OPENRC_SCRIPTCONFD_SRC="install/initservices/conf.d.${BIN_NAME}"
OPENRC_SCRIPT_DEST="/etc/init.d/${BIN_NAME}"
OPENRC_SCRIPTCONFD_DEST="/etc/conf.d/${BIN_NAME}"
ELEVATE="${ELEVATE:-}"

init_system=""
fresh_install=true

pick_elevation() {
	if [ -n "$ELEVATE" ]; then
		return
	fi

	if [ -f "$(which sudo)" ]; then
		ELEVATE="sudo"
	elif [ -f "$(which doas)" ]; then
		ELEVATE="doas"
	elif [ -f "$(which su)" ]; then
		ELEVATE="su"
	else
		echo "Error: no privilege escalation tool found (sudo/doas/su) for some reason." >&2
		echo "Set ELEVATE varible explicitly in the script" >&2
		exit 1
	fi
}

elevate() {
	case "$ELEVATE" in
		sudo|doas) "$ELEVATE" "$@" ;;
		su)        su -c "$(printf '%q ' "$@")" ;;
		*)         echo "Error: unknown \$ELEVATE value: $ELEVATE. Howd you get there?" >&2; exit 1 ;;
	esac
}

parse_args() {
	for arg in "$@"; do
		case "$arg" in
			--init=systemd) init_system="systemd" ;;
			--init=openrc)  init_system="openrc" ;;
			--init=*) echo "Error: unknown init value or init not supported: $arg." >&2; exit 1 ;;
		esac
	done
}

detect_init_system() {
	if [ -n "$init_system" ]; then
		return
	fi

	if [ -f "$(which systemctl)" ]; then
		init_system="systemd"
	elif [ -f "$(which rc-service)" ]; then
		init_system="openrc"
	else
		echo "Error: could not determine init system, pass --init=systemd or --init=openrc. No other inits are supported." >&2
		exit 1
	fi
}

check_existing_install() {
	if [ -f "$BIN_DEST" ]; then
		fresh_install=false
		echo "Existing binary found at $BIN_DEST, upgrading"
	fi
}

build_binary() {
	echo "Building release binary"
	cargo build --release --locked
}

install_binary() {
	echo "Installing binary to $BIN_DEST"
	elevate install -Dm755 "target/release/${BIN_NAME}" "$BIN_DEST"
}

install_config() {
	if [ -d "$CONFIG_DIR" ]; then
		echo "Creating config dir at $CONFIG_DIR"
		elevate mkdir -p "$CONFIG_DIR"
	fi

	if [ -f "$CONFIG_DEST" ]; then
		echo "$CONFIG_DEST exists, leaving it alone"
	elif [ -f "$CONFIG_EXAMPLE" ]; then
		echo "Creating example config at $CONFIG_DEST and $HOME_CONFIG_DEST"
		elevate install -Dm600 "$CONFIG_EXAMPLE" "$CONFIG_DEST"
		install -Dm644 "$CONFIG_EXAMPLE"  "$HOME_CONFIG_DEST"
	else
		echo "Warning: no example config found at $CONFIG_EXAMPLE, skipping, blame the maintainer" >&2
	fi
}

install_systemd_unit() {
	if [ ! -f "$SYSTEMD_UNIT_SRC" ]; then
		echo "Error: $SYSTEMD_UNIT_SRC not found, blame the maintainer" >&2
		exit 1
	fi
	echo "Creating systemd unit at $SYSTEMD_UNIT_DEST and user unit at $HOME_SYSTEMD_UNIT_DEST"
	elevate install -Dm644 "$SYSTEMD_UNIT_SRC" "$SYSTEMD_UNIT_DEST"
	install -Dm644 "$SYSTEMD_UNIT_SRC" "$HOME_SYSTEMD_UNIT_DEST"
	elevate systemctl daemon-reload

	if [ "$fresh_install" == true ]; then
		echo
		echo "Systemd unit created! If you wish to enable it, the command is below."
		echo "systemctl enable --now ${BIN_NAME}"
	else 
		read -p "Fcheckd has been reinstalled, restart the service? y/n: " answer
		if [ "$answer" == "y" ]; then
		  	elevate systemctl try-restart "${BIN_NAME}.service"
		fi
	fi
}

install_openrc_service() {
	if [ ! -f "$OPENRC_SCRIPT_SRC" ]; then
		echo "Error: $OPENRC_SCRIPT_SRC not found, blame the maintainer" >&2
		exit 1
	fi
	echo "Creating openrc script at $OPENRC_SCRIPT_DEST"
	elevate install -Dm755 "$OPENRC_SCRIPT_SRC" "$OPENRC_SCRIPT_DEST"
	echo "Creating openrc conf script at $OPENRC_SCRIPTCONFD_DEST"
	elevate install -Dm644 "$OPENRC_SCRIPTCONFD_SRC" "$OPENRC_SCRIPTCONFD_DEST"

	if [ "$fresh_install" == true ]; then
		echo
		echo "Openrc script created! If you wish to enable it, the commands are below."
		echo "rc-update add ${BIN_NAME} default"
		echo "rc-service ${BIN_NAME} start"
	else
		read -p "Fcheckd has been reinstalled, restart the service? y/n: " answer
		if [ "$answer" == "y" ]; then
			elevate rc-service --ifstarted "${BIN_NAME}" restart
		fi
	fi
}

main() {
	pick_elevation
	parse_args "$@"
	detect_init_system
	check_existing_install
	build_binary
	install_binary
	install_config

	case "$init_system" in
		systemd) install_systemd_unit ;;
		openrc)  install_openrc_service ;;
	esac

	echo "Done!!! Be happy about it."
}

main "$@"
