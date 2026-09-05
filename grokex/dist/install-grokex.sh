#!/usr/bin/env sh
set -eu

archive_root=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
bin_dir=${GROKEX_BIN_DIR:-"${HOME}/.local/bin"}
grokex_home=${GROKEX_HOME:-"${HOME}/.grokex"}
config_path="${grokex_home}/config.toml"

if [ -e "${config_path}" ]; then
  echo "Refusing to overwrite ${config_path}" >&2
  exit 1
fi

mkdir -p "${bin_dir}" "${grokex_home}"
cp "${archive_root}/bin/grokex" "${bin_dir}/grokex"
cp "${archive_root}/bin/grokex-bin" "${bin_dir}/grokex-bin"
cp "${archive_root}/bin/codex-code-mode-host" "${bin_dir}/codex-code-mode-host"
chmod 0755 "${bin_dir}/grokex" "${bin_dir}/grokex-bin" "${bin_dir}/codex-code-mode-host"
if [ -f "${archive_root}/bin/bwrap" ]; then
  cp "${archive_root}/bin/bwrap" "${bin_dir}/bwrap"
  chmod 0755 "${bin_dir}/bwrap"
fi
cp "${archive_root}/config.toml.example" "${config_path}"

echo "Installed Grokex in ${bin_dir}. Set GROK_API_KEY, then run grokex."
