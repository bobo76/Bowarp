#!/usr/bin/env bash
# Install Agent Workspace for the current user from the unpacked release tarball.
#
#   ./install.sh               install, or upgrade an earlier install in place
#   ./install.sh --uninstall   remove it
#
# The binaries always go to the same folder, and the desktop entry runs them from there, so a
# dock pin keeps opening the current version after an upgrade. No sudo needed.
set -euo pipefail

app=agent-workspace
data_home="${XDG_DATA_HOME:-$HOME/.local/share}"
install_dir="$HOME/.local/opt/$app"
bin_dir="$HOME/.local/bin"
desktop_file="$data_home/applications/$app.desktop"
icon_root="$data_home/icons/hicolor"
icon_sizes=(16 32 48 64 128 256 512)

# Refresh the menu and icon caches; desktops that lack the tools rescan on their own.
refresh_caches() {
  if command -v update-desktop-database > /dev/null; then
    update-desktop-database -q "$data_home/applications" || true
  fi
  if command -v gtk-update-icon-cache > /dev/null && [[ -f "$icon_root/index.theme" ]]; then
    gtk-update-icon-cache -q -t "$icon_root" || true
  fi
}

run_uninstall() {
  rm -rf "$install_dir"
  for program in "$app" agentws-hook; do
    # Only our own links: never a same-named program installed some other way.
    if [[ -L "$bin_dir/$program" && "$(readlink "$bin_dir/$program")" == "$install_dir/"* ]]; then
      rm -f "$bin_dir/$program"
    fi
  done
  rm -f "$desktop_file" "$icon_root/scalable/apps/$app.svg"
  for size in "${icon_sizes[@]}"; do
    rm -f "$icon_root/${size}x${size}/apps/$app.png"
  done
  refresh_caches
  echo "Agent Workspace uninstalled."
}

# Copy then rename, so replacing a binary that is running works ("Text file busy" with cp).
install_file() {
  local source="$1" target="$2" mode="$3"
  mkdir -p "$(dirname "$target")"
  install -m "$mode" "$source" "$target.new"
  mv -f "$target.new" "$target"
}

run_install() {
  local source_dir
  source_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
  for file in "$app" agentws-hook "$app.desktop"; do
    if [[ ! -f "$source_dir/$file" ]]; then
      echo "$source_dir/$file is missing; run install.sh from the unpacked release tarball" >&2
      exit 1
    fi
  done

  # agentws-hook must sit next to the app, or agent status is disabled.
  install_file "$source_dir/$app" "$install_dir/$app" 755
  install_file "$source_dir/agentws-hook" "$install_dir/agentws-hook" 755
  mkdir -p "$bin_dir"
  ln -sfn "$install_dir/$app" "$bin_dir/$app"

  # The desktop entry spec quotes the Exec path (it may have spaces) and escapes `"` `` ` `` `$`
  # `\` inside the quotes with a backslash, which the file then writes as two (string escaping).
  # `%` starts a field code, so it is doubled.
  local exec_path="$install_dir/$app"
  exec_path="${exec_path//\\/\\\\\\\\}"
  exec_path="${exec_path//\"/\\\\\"}"
  exec_path="${exec_path//\`/\\\\\`}"
  exec_path="${exec_path//\$/\\\\\$}"
  exec_path="${exec_path//%/%%}"
  mkdir -p "$(dirname "$desktop_file")"
  local line
  while IFS= read -r line || [[ -n "$line" ]]; do
    if [[ "$line" == Exec=* ]]; then
      line="Exec=\"$exec_path\""
    fi
    printf '%s\n' "$line"
  done < "$source_dir/$app.desktop" > "$desktop_file.new"
  mv -f "$desktop_file.new" "$desktop_file"

  for size in "${icon_sizes[@]}"; do
    install_file "$source_dir/icons/$app-$size.png" "$icon_root/${size}x${size}/apps/$app.png" 644
  done
  install_file "$source_dir/icons/$app.svg" "$icon_root/scalable/apps/$app.svg" 644

  refresh_caches
  echo "Agent Workspace installed in $install_dir."
  if [[ ":$PATH:" != *":$bin_dir:"* ]]; then
    echo "Add $bin_dir to PATH to start it with '$app' from a terminal."
  fi
}

case "${1:-}" in
  "") run_install ;;
  --uninstall) run_uninstall ;;
  *)
    echo "usage: $0 [--uninstall]" >&2
    exit 2
    ;;
esac
