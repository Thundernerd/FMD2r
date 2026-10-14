#!/usr/bin/env bash

# Copyright (c) 2021-2026 community-scripts ORG
# Author: Thundernerd
# License: MIT | https://github.com/community-scripts/DevScripts/raw/main/LICENSE
# Source: https://github.com/Thundernerd/FMD2r

source /dev/stdin <<<"$FUNCTIONS_FILE_PATH"
color
verb_ip6
catch_errors
setting_up_container
network_check
update_os

# Upstream Lua modules shell out to python3 and ImageMagick's magick (Debian 13's imagemagick is 7.x).
msg_info "Installing Dependencies"
$STD apt install -y \
  imagemagick \
  python3
msg_ok "Installed Dependencies"

NODE_VERSION="24" setup_nodejs

fetch_and_deploy_gh_release "fmd2r" "Thundernerd/FMD2r" "prebuild" "latest" "/opt/fmd2r" "fmd2r-v*-$(arch_resolve x86_64 aarch64)-linux-gnu.tar.gz"

# The data folder sits outside /opt/fmd2r so updates can wipe the install folder. It is also the
# working directory: the default save-to folder, downloads/, is relative to it.
msg_info "Creating Service"
mkdir -p /opt/fmd2r_data
cat <<EOF >/etc/systemd/system/fmd2r.service
[Unit]
Description=FMD2r manga downloader
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=root
WorkingDirectory=/opt/fmd2r_data
Environment=FMD2R_BIND=0.0.0.0:8080
Environment=FMD2R_DATA_DIR=/opt/fmd2r_data
ExecStart=/opt/fmd2r/fmd2r serve
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF
systemctl enable -q --now fmd2r
msg_ok "Created Service"

motd_ssh
customize
cleanup_lxc
