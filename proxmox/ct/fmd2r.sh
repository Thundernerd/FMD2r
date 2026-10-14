#!/usr/bin/env bash
_CS_DEFAULT_URL="https://raw.githubusercontent.com/Thundernerd/FMD2r/main/proxmox"
_cs_boot="${COMMUNITY_SCRIPTS_CORE_DIR:-$(dirname "${BASH_SOURCE[0]}")/../../core}/core/build.func"
source "$_cs_boot" 2>/dev/null || source <(curl -fsSL "${COMMUNITY_SCRIPTS_CORE_URL:-https://raw.githubusercontent.com/community-scripts/core/main}/core/build.func")

# Copyright (c) 2021-2026 community-scripts ORG
# Author: Thundernerd
# License: MIT | https://github.com/community-scripts/DevScripts/raw/main/LICENSE
# Source: https://github.com/Thundernerd/FMD2r

APP="FMD2r"
var_tags="${var_tags:-manga;downloader}"
var_cpu="${var_cpu:-2}"
var_ram="${var_ram:-1024}"
var_disk="${var_disk:-8}"
var_os="${var_os:-debian}"
var_version="${var_version:-13}"
var_unprivileged="${var_unprivileged:-1}"
# Release tarballs exist for arm64, but this script has not been run on an arm64 host yet.
#var_arm64="${var_arm64:-yes}"

header_info "$APP"
variables
color
catch_errors

function update_script() {
  header_info
  check_container_storage
  check_container_resources

  if [[ ! -f /opt/fmd2r/fmd2r ]]; then
    msg_error "No ${APP} Installation Found!"
    exit
  fi

  if check_for_gh_release "fmd2r" "Thundernerd/FMD2r"; then
    msg_info "Stopping Service"
    systemctl stop fmd2r
    msg_ok "Stopped Service"

    CLEAN_INSTALL=1 fetch_and_deploy_gh_release "fmd2r" "Thundernerd/FMD2r" "prebuild" "latest" "/opt/fmd2r" "fmd2r-v*-$(arch_resolve x86_64 aarch64)-linux-gnu.tar.gz"

    msg_info "Starting Service"
    systemctl start fmd2r
    msg_ok "Started Service"
    msg_ok "Updated successfully!"
  fi
  exit
}

start
build_container
description

msg_ok "Completed Successfully!\n"
echo -e "${CREATING}${GN}${APP} setup has been successfully initialized!${CL}"
echo -e "${INFO}${YW}Access it using the following URL:${CL}"
echo -e "${GATEWAY}${BGN}http://${IP}:8080${CL}"
